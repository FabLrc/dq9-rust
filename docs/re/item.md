# Objets : tables `itemdt_<catégorie>.gp2` et drapeaux « objet obtenu »

Implémentation : `crates/dq9-formats/src/item.rs` (`ItemTable::add_member`,
`ItemTable::compendium`) et `crates/dq9-core/src/trigger.rs` (`State::obtain`) ;
`dq9-core::state::item_obtained_flag`. Données : `data/prm/itemdt_<catégorie>.gp2`, membre
`itemdt_<catégorie>_<langue>.nat` (décompressé). Le wiki DQIX, page *Items*, documente les
mêmes tables ; les nombres ci-dessous ont été vérifiés sur la ROM européenne et contre les
états du premier contrôle.

## Table

Le membre commence par un en-tête de **12 octets** — `u16` nombre d'enregistrements, `u16`
drapeaux (0x8000), deux `u32` inutilisés — suivi de `nombre` enregistrements de **32 octets**.
Les tables d'une catégorie d'équipement continuent après les enregistrements par une table de
statistiques et un bloc de noms (non lus ici). Les neuf catégories, par la lettre du fichier :

| lettre | catégorie | enregistrements (EN) | ids | compendium |
|---|---|---:|---|---|
| `w` | armes | 268 | 19050…20918 | 1–268 |
| `s` | boucliers | 45 | 21000…21397 | 269–313 |
| `h` | casques | 132 | 12170…12917 | 314–445 |
| `b` | armures | 183 | 13000…13794 | 446–628 |
| `a` | gants | 78 | 15000…15299 | 629–706 |
| `u` | jambières | 85 | 16093…16390 | 707–791 |
| `l` | chaussures | 101 | 17091…17421 | 792–892 |
| `d` | accessoires | 52 | 18000…18055 | 893–944 |
| `t` | outils | 234 | 22000…22290 | 0 (2 objets), 945–1176 |

Un enregistrement :

| décalage | type | sens |
|---:|---|---|
| `+0x00` | `u32` | actions d'utilisation (wiki : deux `u16`, 252 = aucune) |
| `+0x08` | `u32` | catégorie (quartet bas = classe d'équipement) |
| `+0x0c` | `u32` | bits 12–22 : **indice de compendium** ; quartet bas : position de tri |
| `+0x18` | `s16` | **id de l'objet** (`func_020de56c`, `0x020de57c`) |

L'indice de compendium est celui que lit `func_0207cc18` (`0x0207cc28`) :
`(record[0xc] & 0x7fffff) >> 0xc`. Il vaut 0 pour les objets sans entrée au compendium
(une partie des outils) : le jeu ne pose alors aucun drapeau.

## Drapeau « objet obtenu »

`func_0207cc18` (`0x0207cc28`, appelée par `Items_Give`, les ajouts au sac et les
équipements) :

```c
compendium = (record[0xc] & 0x7fffff) >> 0xc;
if (compendium != 0 && !Flags_Test(manager, manager + 0x8c, compendium + 0xc76))
    Flags_Set(manager, manager + 0x8c, compendium + 0xc76, 1);
```

Le drapeau est donc **global** (`g_QuestManager+0x8c`), posé à la **première** acquisition
(ou au premier équipement) de l'objet, pas à chaque copie. `dq9-core` reproduit la règle dans
`State::obtain` : le premier exemplaire donne `0xc76 + compendium` ; l'indice vient de
`ItemTable`, remplie par l'application depuis la ROM.

## Première acquisition à l'ouverture

Les quatre drapeaux posés ensemble à l'image **3740** de la course originale sont ceux de
l'équipement de départ, confirmé à la création du personnage :

| id | objet | compendium | drapeau |
|---:|---|---:|---:|
| 20004 | épée de cuivre | 1 | 3191 |
| 13007 | costume céleste | 585 | 3775 |
| 16215 | bas célestes | 765 | 3955 |
| 17120 | souliers célestes | 856 | 4046 |

L'épée vient de la table de création `ov_009` `0x0218636c` (fichier `0x20cc` : 20004, 20500,
20403, 20600, 20100 ; la liste d'`u16` en `0x0218ab78` répète 20004 avec 13007, 16215,
17120). Les trois pièces de tenue sont déjà dans `HeroLook::new_game`. L'auréole (12805,
compendium 398 → 3588) n'est pas dans les drapeaux de `pro1` : ce n'est pas un objet obtenu.

L'écrivain réel de ces quatre drapeaux n'est pas `Item_FlagObtained` mais
`Party_FlagEquippedItems` (`func_0206e424`, EU `0x0206e434`) : une trace RAM (crochet sur
`Flags_Set` écrivant `{lr, indice, valeur}` en `0x020e7300`, relu par `Save_BackupRead`) les
attribue à `lr = 0x0206e524`. Décompilation (Ghidra) : pour un membre 0..3
(`GetPartyMemberByIndex`), la fonction copie la liste d'emplacements d'équipement de
`0x0206e564` (9 octets, terminée par une valeur négative) ; pour chaque emplacement
(`membre + 0x194 + slot * 0x20`) dont la classe (`+8 & 0xf`) est < 8 et le compendium
(`(+0xc & 0x7fffff) >> 12`) non nul, si `Flags_Test(+0x8c, 0xc76 + compendium)` est faux, elle
fait `Flags_Set(+0x8c, 0xc76 + compendium, 1)` et compte ; le compte part ensuite à
`func_020a04c8` [hypothèse : statistique/succès]. Même formule, chemin distinct, sans
`Items_Give`. Appelants (xrefs) : `ov_017::0218b688` (appel `(x, 0)` selon la branche
d'analyse) et `ov_017::021b688c` [hypothèse : boucle sur la liste du groupe
`+0xf78`/`+0xf7c`]. `dq9-core` ne modélise pas ce chemin : `State::obtain`
(`crates/dq9-core/src/trigger.rs`) ne couvre que les objets donnés ; le départ est rendu par
`start_new_game` via la même règle, mais l'équipement d'un compagnon recruté ne posera pas
ses drapeaux [hypothèse sur le moment exact où l'original appelle la fonction au
recrutement].

L'événement de fin de `21596` (`triggerX01.bin`, room 4105, événement 11) donne l'objet
**22159** (benevolessence) par `0x72` : son compendium est 1093, d'où le drapeau **4283**, posé
à l'image 21142 de la course originale.

## Objets des états de contrôle

| drapeau | id | objet | provenance |
|---:|---:|---|---|
| 4135 | 22000 | herbe médicinale | butin du combat 26, aléatoire (course féminine) |
| 4212 | 22087 | slimedrop | butin du combat 26, autre course (`bat6`) |
| 4283 | 22159 | benevolessence | trigger de fin de `21596` (`0x72`) |

`4135` et `4212` ne sont pas des constantes d'ouverture : trois courses réelles avec les mêmes
entrées sauf le sexe donnent l'herbe, ou aucun des deux, et `bat6` donne le slimedrop. C'est
du **butin de combat aléatoire** ; le runtime substitue le combat 26 et ne tire pas de butin
(`melonds-state.md`, § « Oracle différentiel »), d'où l'écart qui reste assumé.
