# Polices

Implémentation : `crates/dq9-formats/src/font.rs`. Outil : `cargo dq9re font me` (planche de
glyphes) ou `cargo dq9re font me --text "Caract<'e>ristiques"` (rendu d'une ligne), PNG dans
`build/`.

## Comment ça a été trouvé

1. Le moteur de la boîte de message (`MsgWin_DrawGlyphs`, `0x02066d00`) appelle
   `Font_Kerning` (`0x020425f4`), qui lit un tableau global de polices.
2. Ce tableau est rempli par `Font_LoadAll` (`0x02042954`) → `Font_Load` (`0x02042814`), qui
   charge `data/pack_lv5/fi_%s.bin` et `fd_%s.bin` pour `s7` puis `me`.
3. La planche décodée (`fd_me.bin`) affiche l'alphabet attendu ; les chaînes du tableau des
   glyphes (`fi_me.bin`) correspondent exactement aux balises d'accent des textes (`<'e>`,
   `<^a>`…). Test `fonts_cover_accent_tags` : toutes les balises d'accent utilisées dans les
   textes des 5 langues sont des glyphes de la police `me`.

Les fichiers `pack/font/*.mes` (528) et `pack_lv5/font_lv5.gp2` (`f8.mes`) sont des polices
partielles de kanji (codes Shift-JIS) héritées de la version japonaise ; elles ne servent pas
au texte européen **[hypothèse : jamais chargées en version EU]**.

| Police | Index | Fichiers | Glyphes | Paires de crénage | Planche | Largeur des espaces |
|---|---|---|---|---|---|---|
| `s7` (petite) | 0 | `fi_s7.bin`, `fd_s7.bin` | 245 | 22 | 1312 × 12 | 2 |
| `me` (messages) | 1 | `fi_me.bin`, `fd_me.bin` | 242 | 105 | 1600 × 12 | 3 |

La largeur des espaces vient de `g_FontSpaceWidth` (`0x020e7aa4`) dans l'ARM9.

## `fi_*.bin` : table des glyphes

```
0x00 "1.1\0"
0x04 u32 nombre de glyphes
0x08 u32 nombre de paires de crénage
0x0C u32 position de la table de crénage
0x10 u32 position de la table des glyphes
0x14 u32 position du bloc de chaînes
```

Les positions sont relatives au début du fichier (le jeu les transforme en pointeurs au
chargement).

Glyphe (8 octets) :

| Offset | Champ |
|---|---|
| 0 | u32 position de la chaîne remplacée par le glyphe (`A`, `<'e>`, `<1>`, `<heart>`…) |
| 4 | u8 largeur en pixels |
| 5 | bits 0-5 : longueur de la chaîne ; bit 6 : voyelle ; bit 7 : minuscule |
| 6 | u16 colonne de départ dans la planche |

Les bits 6 et 7 servent aux règles linguistiques : `<CAP>` met en majuscule le glyphe suivant
s'il est minuscule ; les conditions `<IF_VOWEL_…>` (élision française, « a »/« an ») testent la
voyelle **[hypothèse pour ce dernier point : le test passe par `FUN_02067ffc`, pas encore lu]**.

Crénage (4 octets) : `u8 glyphe gauche, u8 glyphe droit, s8 ajustement, u8 0`.

## `fd_*.bin` : planche de glyphes

```
0x00 "1.0\0"
0x04 u16 largeur, u16 hauteur (pixels)
0x08 u32 taille des données
0x0C u32 position des données
```

Données : 1 bit par pixel, ligne par ligne, bit de poids fort en premier, 1 = encre.

## Mise en page (`Font_MeasureString`, `0x020420f8`)

Pour chaque position du texte :

1. Recherche du **premier** glyphe, dans l'ordre de la table, dont la chaîne est un préfixe du
   texte (`Font_FindGlyph`, `0x020424f4`).
2. Glyphe trouvé : avance = crénage(précédent, glyphe) + largeur + 1, puis on saute la longueur
   de la chaîne.
3. Pas de glyphe (ex. espace) : avance = largeur des espaces + 1, aucun crénage ensuite.

La largeur finale est la somme des avances moins 1. `MsgWin_DrawGlyphs` dessine chaque glyphe à
`x + crénage`, puis avance de `largeur + 1` ; le code `0xFF19` vaut un espace et `0xFF18` passe à
la ligne suivante.
