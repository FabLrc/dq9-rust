# Structure de la ROM (YDQP01, Europe)

Faits observés avec `cargo dq9re info|stats|extract` sur la ROM européenne
(268 435 456 octets, CRC d'en-tête `0xF9CC` valide). Chaque ligne est vérifiable
avec l'outil ; les hypothèses sont marquées **[hypothèse]**.

## Binaires

| Élément | Valeur |
|---|---|
| ARM9 | chargé à `0x02000000`, entrée `0x02000800`, 0x9BD08 octets en ROM → 0xF4618 après décompression BLZ |
| SDK Nitro | `0x04027539` (champ `sdk_version` des module params) |
| Module statique | `0x02000000..0x020F2E60` (puis autoloads) |
| .bss statique | `0x020F2E60..0x021536E0` |
| Autoload ITCM | `0x01FF8000`, 0x1740 octets + 0x5B40 bss |
| Autoload DTCM | `0x027E0000`, 0x60 octets + 0x20 bss |
| ARM7 | `0x02380000`, 0x28FC4 octets (non compressé, probablement le binaire ARM7 standard du SDK **[hypothèse]**) |

## Overlays ARM9

35 overlays, tous compressés BLZ. Ils partagent 6 emplacements mémoire (les overlays
d'un même emplacement sont mutuellement exclusifs) :

| Adresse | Overlays (taille décompressée) |
|---|---|
| `0x021536E0` | 0 (0x30B40), 1 (0x12120), 2 (0x19CC0), 3 (0x2D5C0), 4 (0x1D0E0), 5 (0x9740), 6 (0xCCA0) |
| `0x021842A0` | 7 (0x20), 8 (0x7300), 9 (0x6AC0), 10 (0x820), 11 (0x48E0), 12 (0x7080), 13 (0x3C40), 14 (0x5560) |
| `0x0218B5A0` | 15 (0x8FC0), 16 (0x11B20), 17 (0x4CD40), 18 (0x20), 19 (0xD80), 20 (0x2780), 21 (0x640) |
| `0x021D8A40` | 22 (0x20), 23 (0x26FA0), 24 (0x27700), 25 (0x16F20), 26 (0x64C0), 27 (0x4EE0), 28 (0x10A0), 29 (0x1EA0), 30 (0x1220) |
| `0x02200160` | 31 (0x4C820), 32 (0x20) |
| `0x022A2180` | 33 (0x8A0), 34 (0x8E0) |

Les overlays 7, 18 et 22 ne contiennent que 0x20 octets (une entrée de table
d'initialiseurs statiques) : emplacements « vides » **[hypothèse]**. L'overlay 32 fait aussi
0x20 octets mais réserve 0xA2000 octets de .bss à `0x02200180` (tampon/tas **[hypothèse]**).

Import Ghidra (`scripts/ghidra-import.ps1`) : 12 874 fonctions détectées par l'analyse
automatique (5 692 dans le module statique ARM9, 1 790 dans l'overlay 31, 1 068 dans
l'overlay 17, 622 dans l'overlay 0).

## Système de fichiers

7 481 fichiers nommés (7 516 entrées FAT, les 35 premières étant les overlays), tous sous
`data/` sauf `dwc/` (Nintendo Wi-Fi Connection). Moteur Level-5.

| Ext. | Fichiers | Octets | Magic | Remarques |
|---|---:|---:|---|---|
| gp2 | 1 671 | 76 M | `GPC2` | scripts (`data/scenario`, `data/event`, `data/evspt_*`) **[hypothèse : bytecode d'événements]** |
| sdat | 3 | 69 M | `SDAT` | son Nitro standard (SSEQ/SBNK/SWAR/STRM) |
| chr | 2 683 | 38 M | `NARC` | personnages/effets ; ex. `data/chara_sub/s158.chr` = `.bcfg`, modèle `BMD0` (`.nsbmd`), animations `BCA0` (`.nsbca`), non compressés |
| ambl | 681 | 28 M | `NARC` | cartes ; membres nommés (`.bmbl`, `.dat`, `.nsbtx`) compressés LZ10 |
| amdj | 669 | 22 M | `NARC` | cartes (données complémentaires) |
| mods | 6 | 11 M | `MODS` | vidéos Actimagine Mobiclip |
| spr | 1 300 | 2,9 M | `01 00 03 00` | sprites 2D |
| pac | 126 | 2,3 M | — | archives avec noms en clair |
| bin | 162 | 2,2 M | divers | tables de données |
| stb | 46 | 1,4 M | `SB2.` | **[inconnu]** |
| nsarc / npc / mse | 93 | 1,1 M | `NARC` | |
| nat | 10 | 245 K | — | **[inconnu]** |
| sadl | 4 | 115 K | texte `#def` | |
| lia | 8 | 14 K | `LI5.` | **[inconnu]** |
| NCGR/NCLR/NCER/NANR | 11 | 14 K | Nitro standard | |

Les membres de NARC sont souvent compressés LZ10 (premier octet `0x10`) : par ex.
`data/map/F01M01.ambl` membre 2 se décompresse en `BTX0` (texture Nitro).

## Formats à documenter (par priorité pour le portage)

1. `gp2` — logique du jeu scriptée.
2. Modèles/textures 3D Nitro (`BMD0`/`BTX0`/`BCA0`) dans les NARC `chr`/`ambl`.
3. Tables de données `bin` (objets, monstres, sorts) et textes.
4. `spr`, `pac`, `stb`, `nat`, `lia`.
