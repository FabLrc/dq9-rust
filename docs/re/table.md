# Tables d'enregistrements Level-5 (`.bin`, `.bmmp`, `.bcfg`)

Implémentation : `crates/dq9-formats/src/table.rs`. Outils : `cargo dq9re table <archive.gp2>
<membre>` (affiche les enregistrements), `cargo dq9re text --lang en` (tous les textes).

Format générique de la majorité des données du jeu : dialogues (`scenario/*/NNN_xx.bin`),
menus (`bin/menu`), paramètres, cartes (`map/*.bin`, flottants de positions), etc.
Le nom du format est inconnu **[hypothèse : ancêtre du « cfg.bin » des jeux Level-5 3DS]** ;
le parseur du jeu n'est pas encore localisé dans le code.

## Preuves

Hypothèse construite sur `scenario/M02M0/018_en.bin`, puis vérifiée sur toutes les données
(test `record_tables_parse` de `crates/dq9-formats/tests/real_rom.rs`) :

- 47 386 membres de GPC2 se lisent sans erreur : 47 075 `.bin` (sur 47 227), les 283 `.bmmp`
  et les 28 `.bcfg`. Les 152 `.bin` restants ont d'autres formats (ex.
  `pack_lv5/minimap/shipMPos.bin`).
- Chaque décalage de chaîne pointe sur un début de chaîne, et le mot 3 de l'en-tête est
  exactement le nombre de chaînes distinctes référencées.
- Les valeurs de type 2 sont des flottants plausibles (`map/loola/*.bin` : `-67.27`, `0.17`…).

## Structure

```
0x00 u32 nombre d'enregistrements
0x04 u32 position du bloc de chaînes
0x08 u32 taille du bloc de chaînes
0x0C u32 nombre de chaînes distinctes
0x10 enregistrements…
     octets 0xFF jusqu'au bloc de chaînes
     bloc de chaînes (chaînes terminées par 0), puis bourrage à 16 octets
```

Enregistrement :

```
u16  type (signification propre au fichier)
u8   n = nombre de champs
u8[ceil(n/4)] types des champs, 2 bits par champ, bit de poids faible en premier
     bourrage 0xFF jusqu'à un multiple de 4 octets
u32[n] valeurs
```

| Type | Valeur |
|---|---|
| 0 | chaîne : décalage dans le bloc de chaînes ; `0xFFFFFFFF` = absente |
| 1 | entier 32 bits |
| 2 | flottant 32 bits IEEE |
| 3 | jamais rencontré (rejeté) |

Exemple (`018_en.bin`, en-tête d'enregistrement `01 00 04 15`) : type 1, 4 champs de types
`1,1,1,0` → trois entiers puis une chaîne. Avec 5 champs (`02 00 05 55 00 FF FF FF`) les types
occupent deux octets.

Les chaînes sont en ASCII avec balises `<…>` (voir `text.md`) ; quelques chaînes de debug
sont en Shift-JIS (`bin/presetdt`).

## Dialogues (`scenario/<lieu>/NNN_<langue>.bin`)

Formes d'enregistrements observées (fichiers anglais) : types 1, 2, 4, 5, soit avec 4 champs
`int, int, int, chaîne` (35 376), soit avec 5 champs `int, int, int, int, chaîne` (2 612).
Pour 4 champs : `[a, b, identifiant du message, texte]`. Signification de `a`, `b` et du champ
supplémentaire : **[inconnue — probablement condition/orateur]**.
