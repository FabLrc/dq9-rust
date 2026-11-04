# Archives GPC2 (`*.gp2`) et compression Level-5

Implémentation : `crates/dq9-formats/src/{gpc2,level5}.rs`. Outil :
`cargo dq9re gp2 <chemin>` (liste, `--out` extrait) et `cargo dq9re extract --unpack`
(tout le contenu dans `build/rom/gp2/`).

## Comment ça a été trouvé

1. La chaîne `GPC2` n'apparaît qu'une fois dans l'ARM9 (`0x020f27f8`, table des tags
   `GPC0`/`GPC1`/`GPC2` à `0x020f27e8`).
2. `FileTags_StaticInit` (`0x020e6720`) copie ces tags dans les globales
   `g_TagGpc0..2` (`0x020f27c8..d0`).
3. `g_TagGpc2` n'est lu que par `Gpc2_LoadIndex` (`0x020d91ec`), qui lit un en-tête de
   0x14 octets puis décompresse l'index ; `Gpc_FindEntryByHash` (`0x020d902c`) et
   `Gpc_EntryLocation` (`0x020d8fdc`) donnent le format des entrées.
4. Hypothèses vérifiées sur les 1 671 fichiers `.gp2` de la ROM : tailles, hachages des noms
   et décompression de tous les membres (test `crates/dq9-formats/tests/real_rom.rs`).

## Bloc compressé Level-5

```
u32  method | (decompressed_size << 3)
...  corps
```

| method | Encodage du corps (codecs BIOS Nintendo, sans leur en-tête) |
|---|---|
| 0 | stocké tel quel |
| 1 | LZ10 |
| 2 | Huffman, symboles de 4 bits (quartet bas émis en premier) |
| 3 | Huffman, symboles de 8 bits |
| 4 | RLE |

Méthodes observées (archives sans drapeau) — index : 0 ×1 257, 2 ×403, 1 ×9, 3 ×2 ;
noms : 1 ×1 657, 0 ×14 ; membres : 1 (LZ10) ×42 402, 4 (RLE) ×7 763, 3 ×2 437, 2 ×392,
0 ×32. Contrôles de cohérence : les 758 `.nsbtx` décompressés commencent par `BTX0`, les
textes RLE sont lisibles (`bin/bstr.gp2`).

## Archive GPC2

Toutes les positions sont en **mots de 32 bits**, relatives au début du fichier.

| Offset | Type | Champ |
|---|---|---|
| 0x00 | `[u8;4]` | `GPC2` |
| 0x04 | u16 | bits 0-11 : nombre d'entrées ; bits 12-15 : profondeur de la recherche dichotomique (`ceil(log2(n))`) |
| 0x06 | u16 | position de l'index (toujours 5 = juste après l'en-tête) |
| 0x08 | u16 | position de la table des noms |
| 0x0A | u16 | position de la zone de données |
| 0x0C | u16 | taille décompressée de l'index |
| 0x0E | u16 | taille décompressée de la table des noms |
| 0x10 | u32 | bits 0-27 : taille de la zone de données ; bits 28-31 : drapeaux |

- Index et table des noms sont chacun un bloc Level-5.
- La zone de données va jusqu'à la fin du fichier (`(0x0A + taille) * 4 == taille du fichier`).
- Drapeau `0x1` (bit 28) : membres stockés **sans** en-tête Level-5. Vu seulement dans
  `pack_lv5/enemy.gp2` et `prm/actdt_a.gp2`/`actdt_b.gp2`.
- Au chargement, le jeu remplace le tag par `GPC1` pour la version décompressée en mémoire
  (`Gpc1_FindMember`, `0x020d9110`) **[hypothèse : `GPC0` = variante non compressée]**.

### Entrée d'index (12 octets, triées par hachage croissant)

| Offset | Champ |
|---|---|
| 0x0 | CRC-32 (zlib) du nom du membre |
| 0x4 | bits 0-23 : position du membre (mots) relative à la zone de données ; bits 24-31 : octet bas de la position du nom |
| 0x8 | bits 0-23 : taille stockée en octets ; bits 24-31 : octet haut de la position du nom |

Position du nom (octets dans la table des noms) = `(entry[0x8] >> 24) << 8 | entry[0x4] >> 24`.
Les noms sont terminés par un zéro.

## Contenu

53 639 membres dans 1 671 archives. Extensions : `.bin` (47 227), `.nsbmd` (1 913),
`.nsbtx` (758), `.stb` (689), `.nat` (571), `.mes` (529), `.pac` (375), `.bmmp` (283),
`.obg` (268), `.chr` (169), …

Exemple : `data/scenario/M02M0.gp2` contient `NNN_{de,en,es,fr,it}.bin`, les textes de dialogue
par langue (voir `docs/re/text.md`).
