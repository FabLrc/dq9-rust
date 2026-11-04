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
      conditions principales, action « parler au PNJ » (`trigger.md`) ; `dq9re talk`
      enchaîne triggers et choix de la réplique.

## Prochaines étapes

1. **Triggers, suite** : conditions restantes (0x29, 0x31, 0x2A…), autres actions (0x77
   et suivantes : `Trig_RunDeferred`, `Trig_RunAction`), types d'événements autres que 0.
2. **Scripts d'événements** : `.stb` (`SB2`) dans `event/` et `scenario/`.
3. **Graphismes 3D Nitro** (`BMD0`/`BTX0`/`BCA0`, formats documentés publiquement) : lecteur
   + rendu wgpu de premier jet (personnages `chr`, cartes `ambl`).
4. **Tables de jeu** : objets, monstres, sorts (`bin/*`, `prm/*`, `.nat` binaires) ; noms
   d'objets/monstres pour la flexion grammaticale (`FUN_020e4968`).
5. **Frontend** `dq9-app` (winit + wgpu, natif et wasm) affichant d'abord une carte et une
   boîte de dialogue réelles (cadre de fenêtre, nom de l'orateur `//Nom//`, défilement).
6. Balises non localisées : `<RIKKA>`, `<CEN>`, `<TC#>`, `<W=#>`… (`text.md`).
