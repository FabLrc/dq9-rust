# Feuille de route

État au fil des commits ; chaque étape se termine par une doc dans `docs/re/`, une
implémentation testée et un commit.

## Fait

- [x] Environnement : Rust, CI, outils RE (Ghidra + import scripté, melonDS), `dq9re`.
- [x] Conteneur ROM : en-tête, fichiers, ARM9/overlays (BLZ), NARC, codecs Nintendo.
- [x] Archives GPC2 + compression Level-5 (`gpc2.md`).
- [x] Tables d'enregistrements (`table.md`) ; extraction de tous les textes (`text.md`).
- [x] Pipeline des messages : tables de balises, codes de contrôle `FF01..FF2F`.

## Prochaines étapes

1. **Police et rendu du texte** : fichiers `pack/font/*.mes` (528 membres, format inconnu),
   correspondance balises d'accent → glyphes, puis `Msg_Compile` en Rust.
2. **Sens des champs des dialogues** (`a`, `b` des enregistrements de `scenario/`) : suivre
   le code qui lit `NNN_xx.bin` (chemin `data/scenario/trigger%s.bin` à `0x020f05xx`).
3. **Scripts d'événements** : `.stb` (`SB2`) dans `event/`, et triggers de scénario.
4. **Graphismes 3D Nitro** (`BMD0`/`BTX0`/`BCA0`, formats documentés publiquement) : lecteur
   + rendu wgpu de premier jet (personnages `chr`, cartes `ambl`).
5. **Tables de jeu** : objets, monstres, sorts (`bin/*`, `prm/*`, `.nat` binaires).
6. **Frontend** `dq9-app` (winit + wgpu, natif et wasm) affichant d'abord une carte et une
   boîte de dialogue réelles.
