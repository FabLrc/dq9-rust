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

L'analyse automatique initiale de Ghidra avait détecté 12 874 fonctions (5 692 dans le
module statique ARM9, 1 790 dans l'overlay 31, 1 068 dans l'overlay 17, 622 dans l'overlay 0).
Les imports appliquent désormais aussi les bornes et noms de la décompilation :
voir ci-dessous. Ces chiffres de détection ne mesurent pas le code réimplémenté.

### Noms et bornes dans Ghidra

Ordre de `scripts/ghidra-import.ps1` :

1. `NdsLayout.java` construit les blocs ; Ghidra exécute son analyse automatique.
2. `ApplyFunctions.java` applique `re/functions.txt` : adresses européennes, taille en
   octets, catégorie et nom, issus des symboles de la
   [décompilation USA](https://github.com/DQIX/dqix-decomp) (Unlicense ; correspondance
   d'adresses dans `../ressources-externes.md`).
3. `ApplySymbols.java` applique `re/symbols.txt` : **nos noms et commentaires priment**.

L'étape 1 est une première invocation headless, sauvegardée avant les étapes 2 et 3,
exécutées dans une seconde invocation avec `-noanalysis`. Sans cette séparation,
Ghidra lance une analyse après les post-scripts et peut élargir à nouveau les corps :
constaté sur `ov_017::021b9e30` lors d'un import neuf.

Pour un projet existant, `scripts/ghidra-apply-symbols.ps1` exécute les étapes 2 et 3
sans réanalyse. Les deux lanceurs acceptent `DQ9_GHIDRA_PROJECT`, comme les requêtes,
pour travailler sur une copie. Fermer le projet dans l'interface avant utilisation.
Attention : l'import complet utilise `-overwrite` ; il reconstruit `arm9.bin`.

Les tailles décrivent des intervalles `[entrée, entrée + taille)` qui peuvent inclure
des pools de constantes, pas seulement des instructions. L'importeur valide tout le
fichier (syntaxe, catégories, adresses uniques, plages dans un bloc initialisé), puis
applique les corps. Il conserve les métadonnées des fonctions aux entrées connues et
les fonctions hors des plages importées. Les fonctions déduites par l'analyse dont
l'entrée est à l'intérieur d'une autre plage importée sont supprimées pour libérer
cette plage ; leurs instructions ne sont pas supprimées.

Exceptions explicites dans l'inventaire actuel :

- Deux tailles nulles en `ov_016::021909cc` et `ov_016::0219c514` : appliquer le nom sans
  inventer de corps (label si aucune fonction n'existe déjà).
- La plage déclarée de `0x020c0eac` finit à `0x020c1140`, après l'entrée suivante
  `0x020c112c`. Ghidra n'autorise pas de corps de fonctions partagés : l'importeur borne
  cette plage à `0x020c112c` exclu et le signale. Ce choix d'import ne tranche pas
  l'origine du chevauchement dans la carte source.
- Un label local préexistant au même endroit et de même nom qu'une fonction importée
  est remplacé par le nom de fonction lors de l'application des symboles locaux.

Le fichier n'indique pas ARM/Thumb : **aucun mode n'est deviné, aucun désassemblage
supplémentaire n'est lancé**. Une donnée définie ou une instruction décodée à cheval sur
une entrée connue est effacée en tant qu'unité de listing pour permettre la création
de la fonction, sans changer les octets ni le contexte processeur. La présence d'un
corps importé n'est donc pas une preuve de désassemblage complet ni de bonne
décompilation. Les marqueurs `port:` restent réservés aux réimplémentations effectives.

Contrôle sur une copie du projet européen et après un import neuf : 14 778 corps et
deux noms sans bornes appliqués, puis 332 symboles locaux. Relecture de tous les noms
et corps sauvegardés, y compris les priorités locales ; deuxième application réussie,
sans nouvelle suppression de fonction inférée. Un fichier dont la seconde entrée
sort des blocs mémoire est rejeté avant toute mutation de la première entrée.
Les lanceurs exigent les bilans de succès et refusent les erreurs Ghidra, qui peuvent
survenir même avec un code de sortie natif 0.

## Système de fichiers

7 481 fichiers nommés (7 516 entrées FAT, les 35 premières étant les overlays), tous sous
`data/` sauf `dwc/` (Nintendo Wi-Fi Connection). Moteur Level-5.

| Ext. | Fichiers | Octets | Magic | Remarques |
|---|---:|---:|---|---|
| gp2 | 1 671 | 76 M | `GPC2` | archives Level-5 de fichiers nommés et compressés, 53 639 membres (textes, tables, modèles…) — voir `gpc2.md` |
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

1. Contenu des archives `gp2` : textes (`.bin` de `scenario/`, `.mes`), tables, scripts.
2. Modèles/textures 3D Nitro (`BMD0`/`BTX0`/`BCA0`) dans les NARC `chr`/`ambl`.
3. Tables de données `bin` (objets, monstres, sorts) et textes.
4. `spr`, `pac`, `stb`, `nat`, `lia`.
