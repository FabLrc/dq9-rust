# Liste des cartes (`data/map/maplist9.bin`)

Index de toutes les cartes : l'id d'une carte (celui des triggers, des placements, des
portes et de `g_Zone3D+0`) mène à son code, qui nomme ses archives (`<code>.ambl`,
`<code>.amdj`). Implémentation : `dq9_formats::maplist` ; `cargo dq9re maps` liste les
cartes, `cargo dq9re maps --state <état.ml1>` les compare à la RAM.

## Lecture par le jeu

`MapList_Load` (`0x020998d4`) passe le fichier, une table d'enregistrements (`table.md`),
aux gestionnaires `g_MapListRecordHandlers` (`0x020f1524`) :

| type | gestionnaire | effet |
|---|---|---|
| `0x66` | `MapList_RecCount` (`0x020995d0`) | alloue `n × 16` octets, capacité = `n` (1 010) |
| `0x67` | `MapList_RecMap` (`0x02099608`) | 22 valeurs → un enregistrement de 16 octets, ajouté (`MapList_Append`, `0x02099a00`) s'il a un code et s'il reste de la place |
| `0x64`, `0x65`, `0x68`, `0x69` | rien | |

La liste vit en `GameState+0x468` (`GameState_MapList`, `0x02011594`) : pointeur,
nombre, capacité. `MapList_Find` (`0x02099960`) rend le **premier** enregistrement de cet
id ; les 139 cartes non livrées ont toutes l'id 0.

## Enregistrement (16 octets)

« Valeur n » = n-ième valeur de l'enregistrement de type `0x67` (lue avec `Tbl_FieldInt`,
`Tbl_FieldStr`).

| octets | valeur | sens |
|---|---|---|
| `+0` u16 | 0 | id de la carte |
| `+2` bits 0-14 | 1 | zone partagée par les cartes d'un lieu (`Zone3D+4` ; Stornway = 198) |
| `+4` bit 0 | 21 | [inconnu] (4 cartes) ; bits 1-7 non initialisés |
| `+5..+0xb` | 4 (chaîne) | code de la carte, 7 octets copiés, `+0xb` = 0 : 6 caractères au plus |
| `+0xc` bits 0-3 | 3 | genre de carte, ci-dessous |
| `+0xc` bit 4 | 9 | copié en `func_0208a9b4()+0x9c` par `Zone3D::SwitchZone` [inconnu] |
| `+0xc` bits 5-6, bit 7 | 10, 12 | [inconnu] |
| `+0xd` bits 0-6 | 6 | passé par `0x0209cb00` dans une table de 47 octets (avec des cas selon `LightingManager+0x98`) ; [hypothèse, wiki Map-List] la musique |
| `+0xd` bit 7 | 16 | [inconnu] |
| `+0xe` bits 0-1 | 17 | espace : 1 intérieur, 2 extérieur, 0 aucun (wiki) |
| `+0xe` bits 2-5 | 18 | 8 sur 930 cartes ; [hypothèse, wiki] sol d'un décor de combat |
| `+0xe` bits 6, 7 | 19, 20 | [inconnu] |
| `+0xf` | — | jamais écrit |

Les valeurs 2 (région) et 5 (libellé du développeur) sont lues puis jetées ; 7, 8, 11, 13,
14 et 15 ne sont pas lues ici (le wiki rattache 14 et 15 à la position de la carte dans le
monde, lues ailleurs).

**Genre (`+0xc` bits 0-3)**, réparti selon la première lettre du code : 0 = 64 régions de
terrain `F` (+ 9 autres), 1 = donjons `D`, châteaux, tours (157), 2 = villes, maisons,
lieux d'histoire `M`/`C`/`H`/`S`/`X` (383), 4 = décors de combat `B` (182), 6 = grottes
`Z` (165), 5 et 8 = codes `R`, 7 = `O00`/`O01`, 3 = une tour. Quand il vaut 0,
`Zone3D::SwitchZone` (`0x0201384c`) copie l'id de la carte dans l'objet du protagoniste
(`+0x566`) : [hypothèse] la dernière région de terrain visitée.

## Vérification

- `Zone3D::SwitchZone` : `g_Zone3D+0` = id courant, `+2` = id précédent, `+4` = zone,
  `+8` = pointeur vers l'enregistrement.
- Sur les états melonDS de la démo (`melonds-state.md`), les 1 010 enregistrements calculés
  sont identiques à ceux de la RAM, hors octets non définis (`MapRecord::to_bytes`) ; la
  carte courante est 109, `C01M09`, « Stornway / Church », zone 198.
- Le wiki DQIX ([Map-List](https://github.com/DQIX/wiki/blob/main/Map-List.md)) numérote
  les valeurs de la même façon ; ses lectures de la musique, de l'espace et du sol sont
  reprises comme hypothèses, le reste est lu dans le code ci-dessus.
