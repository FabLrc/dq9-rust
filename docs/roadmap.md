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

## Prochaines étapes

1. **Sens des champs des dialogues** (`a`, `b` des enregistrements de `scenario/`) : suivre
   le code qui lit `NNN_xx.bin` (chaîne `data/scenario/trigger%s.bin` à `0x020f05dc`).
2. **Scripts d'événements** : `.stb` (`SB2`) dans `event/`, et triggers de scénario.
3. **Graphismes 3D Nitro** (`BMD0`/`BTX0`/`BCA0`, formats documentés publiquement) : lecteur
   + rendu wgpu de premier jet (personnages `chr`, cartes `ambl`).
4. **Tables de jeu** : objets, monstres, sorts (`bin/*`, `prm/*`, `.nat` binaires) ; noms
   d'objets/monstres pour la flexion grammaticale (`FUN_020e4968`).
5. **Frontend** `dq9-app` (winit + wgpu, natif et wasm) affichant d'abord une carte et une
   boîte de dialogue réelles (cadre de fenêtre, nom de l'orateur `//Nom//`, défilement).
6. Balises non localisées : `<RIKKA>`, `<CEN>`, `<TC#>`, `<W=#>`… (`text.md`).
