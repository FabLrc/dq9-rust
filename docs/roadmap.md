# Feuille de route

État au fil des commits ; chaque étape se termine par une doc dans `docs/re/`, une
implémentation testée et un commit.

## Fait

- [x] Environnement : Rust, CI, outils RE (Ghidra + import scripté, melonDS), `dq9re`.
- [x] Conteneur ROM : en-tête, fichiers, ARM9/overlays (BLZ), NARC, codecs Nintendo.
- [x] Archives GPC2 + compression Level-5 (`gpc2.md`).
- [x] Tables d'enregistrements (`table.md`) ; extraction de tous les textes (`text.md`).
- [x] Pipeline des messages : tables de balises, codes de contrôle `FF01..FF2F`.
- [x] Polices `fi_*/fd_*` (glyphes nommés par chaîne, crénage) (`font.md`).
- [x] Pipeline des messages réimplémenté (6 passes, conditions, majuscules) et rendu des
      dialogues avec la police du jeu (`dq9re msg`).
- [x] Choix de la réplique d'un PNJ : types d'enregistrements, étapes, quêtes, nuit,
      emplacements et zones de conversation ; lecteur de tables du jeu (`dialogue.md`,
      `dq9re talk`).
- [x] Triggers de scénario : chargement par salle et étape, compilation des mots, choix,
      toutes les conditions des données, pistes de scénario, types d'événements, actions
      immédiates et différées sur un état modélisé (`trigger.md`) ; `dq9re talk` enchaîne
      triggers, actions et choix de la réplique.
- [x] Scripts d'événements `.stb` (`SB2`) : format, machine à pile, désassembleur ; natives
      numérotées (`stb.md`, `dq9re stb`).

## Prochaines étapes

1. **Natives des scripts** : rôle des 228 natives employées (décompilation de
   `ov_001::0215ae1c…`), à commencer par les plus fréquentes (210, 570, 206, 208, 571).
2. **Interpréteur de scripts** dans `dq9-core` : exécuter les `.stb` sur l'état des
   triggers (attentes, messages, drapeaux).
3. **Graphismes 3D Nitro** (`BMD0`/`BTX0`/`BCA0`, formats documentés publiquement) : lecteur
   + rendu wgpu de premier jet (personnages `chr`, cartes `ambl`).
4. **Tables de jeu** : objets, monstres, sorts (`bin/*`, `prm/*`, `.nat` binaires) ; noms
   d'objets/monstres pour la flexion grammaticale (`FUN_020e4968`).
5. **Frontend** `dq9-app` (winit + wgpu, natif et wasm) affichant d'abord une carte et une
   boîte de dialogue réelles (cadre de fenêtre, nom de l'orateur `//Nom//`, défilement).
6. Balises non localisées : `<RIKKA>`, `<CEN>`, `<TC#>`, `<W=#>`… (`text.md`).
7. Points de sens inconnu des triggers : journal de quêtes (conditions 0x25/0x33, bits des
   entrées), événements 6, 19, 22, 23, 27, valeurs globales des conditions 0x5d–0x63.
