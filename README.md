# DQ9 Rust

Réimplémentation en Rust de *Dragon Quest IX – Sentinels of the Starry Skies* (Nintendo DS),
pour Windows, macOS, Linux et le Web, avec à terme le multijoueur de la DS rendu possible en
ligne. Le jeu est rétro-analysé et ses ressources sont lues depuis **votre propre ROM** ; aucune
donnée du jeu n'est incluse dans ce dépôt.

**[Tester DQ9 Rust dans le navigateur](https://fablrc.github.io/dq9-rust/?newgame=1)** —
sélectionnez votre propre ROM européenne (`YDQP01`) : le bouton « Nouvelle partie » s'active
une fois la ROM lue et le jeu prend le clavier automatiquement. Les fichiers ROM et
sauvegarde restent locaux : aucun envoi au serveur.
La démo est publique, expérimentale et nécessite un navigateur récent avec WebGPU ou WebGL 2.

**État** : une première preuve technique se joue, en natif comme dans un navigateur.
`cargo run -p dq9-app --release -- --new-game` lance l'ouverture et enchaîne les scènes du
prologue jusqu'au contrôle libre à l'Observatoire. `--map M01`, ou une sauvegarde passée en
`--save`, ouvre directement Angel Falls. On y trouve :

- deux écrans DS (empilés, côte à côte ou écran du haut seul) ;
- cartes texturées, éclairage, brouillard et heure du jour ;
- PNJ animés et héros animé ;
- déplacement porté en virgule fixe, avec collisions et caméra L/R ;
- portes et maisons, conversations dans la fenêtre de message du jeu ;
- scènes scriptées : interpréteur `.stb`, personnages, caméra, fondus, effets ;
- musique, effets sonores et jingles ;
- départ depuis une sauvegarde DS (`--save`).

Manquent encore :

- les combats : le premier combat scripté est compté comme gagné ;
- quelques effets de scène ;
- la comparaison image par image à melonDS, qui est le critère de la v0.1.

Plan : `docs/roadmap.md` ; lancement et commandes : `docs/app.md`.

<!-- coverage:start -->
**Avancement : 2,1 %** des fonctions de jeu traduites (262 / 12 056), 2,7 % du code (70 Ko / 2 505 Ko).
Calculé par `cargo dq9re coverage` à partir des marqueurs `port:` du code (définition dans `docs/roadmap.md`).

<details><summary>Détail par module</summary>

| Module | Fonctions traduites | Code traduit |
|---|---:|---:|
| ARM9 | 233 / 5 464 (4,2 %) | 61 Ko / 826 Ko (7,2 %) |
| ITCM | 0 / 34 (0,0 %) | 0 Ko / 6 Ko (0,0 %) |
| ov_000 | 0 / 826 (0,0 %) | 0 Ko / 190 Ko (0,0 %) |
| ov_001 | 0 / 515 (0,0 %) | 0 Ko / 70 Ko (0,0 %) |
| ov_002 | 0 / 213 (0,0 %) | 0 Ko / 101 Ko (0,0 %) |
| ov_003 | 0 / 510 (0,0 %) | 0 Ko / 176 Ko (0,0 %) |
| ov_004 | 0 / 532 (0,0 %) | 0 Ko / 113 Ko (0,0 %) |
| ov_005 | 0 / 89 (0,0 %) | 0 Ko / 38 Ko (0,0 %) |
| ov_006 | 0 / 132 (0,0 %) | 0 Ko / 51 Ko (0,0 %) |
| ov_008 | 0 / 63 (0,0 %) | 0 Ko / 29 Ko (0,0 %) |
| ov_009 | 0 / 43 (0,0 %) | 0 Ko / 26 Ko (0,0 %) |
| ov_010 | 0 / 3 (0,0 %) | 0 Ko / 2 Ko (0,0 %) |
| ov_011 | 0 / 186 (0,0 %) | 0 Ko / 18 Ko (0,0 %) |
| ov_012 | 0 / 71 (0,0 %) | 0 Ko / 28 Ko (0,0 %) |
| ov_013 | 0 / 40 (0,0 %) | 0 Ko / 15 Ko (0,0 %) |
| ov_014 | 0 / 69 (0,0 %) | 0 Ko / 21 Ko (0,0 %) |
| ov_015 | 0 / 103 (0,0 %) | 0 Ko / 34 Ko (0,0 %) |
| ov_016 | 0 / 87 (0,0 %) | 0 Ko / 22 Ko (0,0 %) |
| ov_017 | 29 / 1 194 (2,4 %) | 10 Ko / 301 Ko (3,1 %) |
| ov_020 | 0 / 18 (0,0 %) | 0 Ko / 9 Ko (0,0 %) |
| ov_021 | 0 / 5 (0,0 %) | 0 Ko / 2 Ko (0,0 %) |
| ov_023 | 0 / 827 (0,0 %) | 0 Ko / 147 Ko (0,0 %) |
| ov_024 | 0 / 573 (0,0 %) | 0 Ko / 152 Ko (0,0 %) |
| ov_025 | 0 / 304 (0,0 %) | 0 Ko / 90 Ko (0,0 %) |
| ov_026 | 0 / 24 (0,0 %) | 0 Ko / 24 Ko (0,0 %) |
| ov_027 | 0 / 87 (0,0 %) | 0 Ko / 20 Ko (0,0 %) |
| ov_028 | 0 / 34 (0,0 %) | 0 Ko / 5 Ko (0,0 %) |
| ov_033 | 0 / 5 (0,0 %) | 0 Ko / 3 Ko (0,0 %) |
| ov_034 | 0 / 5 (0,0 %) | 0 Ko / 3 Ko (0,0 %) |

</details>
<!-- coverage:end -->

## Arborescence

| Chemin | Rôle |
|---|---|
| `crates/nds-rom` | ROM DS : en-tête, FNT/FAT, overlays, ARM9 (BLZ), LZ10/LZ11/Huffman/RLE, NARC, modèles et animations G3D. Sans E/S, compile en wasm. |
| `crates/dq9-formats` | Formats Level-5 de DQ9 : archives `.gp2`, tables, polices, messages, répliques, triggers, scripts `.stb`, cartes, personnages, éclairage, sauvegarde, SDAT. Sans E/S, compile en wasm. |
| `crates/dq9-core` | Logique de jeu déterministe : état depuis une sauvegarde ou une nouvelle partie, déplacement et collisions en virgule fixe, caméra, portes et zones, triggers, conversations, interpréteur `.stb`, scènes, enchaînement du scénario. Sans E/S, compile en wasm. |
| `crates/dq9-render` | Rendu wgpu proche du moteur 3D de la DS : cartes, PNJ, héros, éclairage, brouillard, interface 2D. Sans E/S, compile en wasm. |
| `crates/dq9-audio` | Séquenceur SSEQ, banques, ondes et mixage du son de la DS. Sans E/S, compile en wasm. |
| `crates/dq9-app` | Application (winit + wgpu, natif et Web) : `cargo run -p dq9-app --release -- --new-game` (`docs/app.md`). |
| `crates/dq9re` | CLI de rétro-ingénierie (`cargo dq9re --help`). |
| `crates/testrom` | Localise la ROM pour les tests sur données réelles. |
| `re/symbols.txt` | Noms retrouvés, source de vérité appliquée au projet Ghidra. |
| `re/ghidra_scripts/`, `scripts/` | Scripts Ghidra et PowerShell : installation des outils, import, requêtes, symboles, états melonDS. |
| `docs/` | Feuille de route, architecture, ressources externes, notes de RE (`re/`). |
| `rom/` | Votre ROM (ignorée par git). |

## Mise en place

ROM attendue : version européenne `YDQP01` (voir `rom/README.md`), placée dans `rom/`, à la
racine, ou désignée par `--rom <chemin>` / `DQ9_ROM`.

Prérequis : Rust via rustup (version épinglée par `rust-toolchain.toml`, cible
`wasm32-unknown-unknown` incluse). Sous Windows, hôte `x86_64-pc-windows-gnu` avec MinGW-w64
(WinLibs) dans le `PATH`.

```powershell
cargo dq9re info                 # vérifie la ROM
cargo dq9re extract --unpack     # code + données dans build/rom, archives .gp2 dépliées

# Outils de RE (Windows) : JDK, Ghidra, melonDS dans ./tools, SHA-256 vérifiés
powershell -ExecutionPolicy Bypass -File scripts/setup-re-tools.ps1
powershell -ExecutionPolicy Bypass -File scripts/ghidra-import.ps1   # projet re/ghidra (~3 min)
powershell -ExecutionPolicy Bypass -File scripts/ghidra.ps1          # interface graphique
powershell -ExecutionPolicy Bypass -File scripts/melonds-state.ps1 -Save partie.sav   # état .ml1 vérifié (~40 s)
```

## Développement

```powershell
powershell -ExecutionPolicy Bypass -File scripts/verify.ps1          # fmt, clippy, tests, build wasm
powershell -ExecutionPolicy Bypass -File scripts/verify.ps1 -Fast    # fmt et clippy seulement
cargo test --workspace                        # tests sur la vraie ROM inclus si elle est présente
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo build -p nds-rom -p dq9-formats -p dq9-core -p dq9-render -p dq9-app --target wasm32-unknown-unknown
cargo dq9re coverage --readme README.md       # avancement (sans ROM), réécrit le bloc ci-dessus
```

`scripts/verify.ps1` enchaîne les contrôles dans l'ordre de la CI et s'arrête au premier
échec ; les cibles partagées entre worktrees et les outils facultatifs (`sccache`,
`cargo nextest`) sont décrits dans [`docs/app.md`](docs/app.md#boucle-de-développement).

Deux outils de diagnostic, sans effet sur le jeu :

```powershell
# Liste (puis supprime avec -Apply) les vieux états d'émulateur sous build/
powershell -ExecutionPolicy Bypass -File scripts/prune-build.ps1
# Compare deux runs par point d'histoire, pas par tick
powershell -ExecutionPolicy Bypass -File scripts/compare-runs.ps1 `
    -Original build/oracle/original.jsonl -Runtime build/oracle/runtime.jsonl
```

`prune-build.ps1` ne touche ni aux `.sav`, ni aux `.nds`, ni aux JSON ou images, et refuse la
racine du dépôt. `compare-runs.ps1` aligne les checkpoints JSONL sur la carte et la piste
courante, puis compare progression, drapeaux et quêtes ; il complète `oracle-compare.ps1`, qui
part d'un état melonDS unique.

La CI exécute fmt, clippy, les tests sous Linux et la compilation wasm, sans jamais avoir accès
à la ROM, puis calcule l'avancement et met à jour ce README sur `main`. Un commit qui ne touche
que la documentation, `re/` ou `rom/` ne relance pas la CI, et le déploiement Pages n'est
redéclenché que par les chemins qui changent le binaire web (`crates/**`, manifestes,
`rust-toolchain*`).

## Rétro-ingénierie

- **`cargo dq9re`** : `ls data/map/`, `gp2 <archive> --out <dossier>`, `table <archive>
  <membre>`, `msg`, `font`, `narc <fichier> -d`, `cat <fichier> -d`,
  `talk data/scenario/M02M0.gp2 018_en.bin --quest 11=1 --level 25` (triggers puis réplique
  choisie pour un état donné), `stb data/event/ev03030.gp2` (script d'événement désassemblé
  avec ses textes), `map X01M07 --image build/scratch/x01m07.png --scale 16` (vue du dessus
  de la collision), `map X01M07 --reach 0 0.17 6.8 --to 0 0.17 17.5 --reach-image
  build/scratch/reach.png` (portée réelle en 3D, voir
  [`docs/re/map-archive.md`](docs/re/map-archive.md#voir-la-collision-dq9re-map---image---reach)).
- **Ghidra** : ARM9 dans le bloc `arm9`, ITCM/DTCM à leurs adresses d'exécution, chaque
  overlay dans son espace `ov_NNN`. Requêtes sans interface :
  `scripts/ghidra-query.ps1 decomp|xrefs|calls|disasm <adresse>` (`0x020d91ec` ou
  `ov_017::0218c0f4`) ; une requête verrouille le projet, en lancer plusieurs demande des
  copies de `re/ghidra` désignées par `DQ9_GHIDRA_PROJECT`. Les résultats sont mis en cache
  dans `build/ghidra-cache/` (clé : commande, adresses, ROM, SHA-256 de `Query.java`) ;
  `-Refresh` force la requête.
  Les imports et `scripts/ghidra-apply-symbols.ps1` appliquent les noms et bornes de
  `re/functions.txt`, puis les surcharges de `re/symbols.txt` ; détails et limites
  dans [`docs/re/rom-layout.md`](docs/re/rom-layout.md#noms-et-bornes-dans-ghidra).
- **melonDS** (`tools/melonds`) : référence visuelle et analyse dynamique par son stub GDB.
- **Avant d'analyser** : symboles et code de la [décompilation USA](https://github.com/DQIX/dqix-decomp)
  (adresses à convertir pour la ROM européenne) et [wiki des formats DQIX](https://github.com/DQIX/wiki),
  voir `docs/ressources-externes.md`.

Consigner une découverte :

1. Nommer fonctions et données dans `re/symbols.txt` (`<adresse> <nom> # commentaire`), puis
   `scripts/ghidra-apply-symbols.ps1` (projet fermé dans l'interface). Les noms locaux
   priment sur ceux de la décompilation ; le projet Ghidra est jetable.
2. Décrire le format ou le comportement dans `docs/re/<sujet>.md` : preuves (adresses,
   contrôles sur données), hypothèses marquées **[hypothèse]**.
3. Implémenter dans la crate correspondante avec un test sur données synthétiques et, si
   pertinent, un invariant sur la vraie ROM (`tests/real_rom.rs`).

### Accès rapide au code d'origine

`scripts/decomp.ps1` relie une adresse ou un nom européens au fichier source de la
décompilation USA (Unlicense, checkout non commité, attendu dans `tools/dqix-decomp`) :

```powershell
powershell -ExecutionPolicy Bypass -File scripts/decomp.ps1 0x02010250   # GameState::AdvanceDayTimer
powershell -ExecutionPolicy Bypass -File scripts/decomp.ps1 GameState::SetDayTimer -Context 80
powershell -ExecutionPolicy Bypass -File scripts/decomp.ps1 -List SetDayTimer
```

Il affiche l'adresse et le nom européens, l'adresse USA, le chemin source et un extrait ;
`-Decomp PATH` désigne un autre checkout. Détails, cache Ghidra et limites :
[`docs/re/outils.md`](docs/re/outils.md).

## Documentation

| Fichier | Contenu |
|---|---|
| `docs/roadmap.md` | jalons jusqu'au portage complet, définition du « 100 % » |
| `docs/architecture.md` | crates cibles et règles (portabilité, déterminisme, traçabilité) |
| `docs/app.md` | lancer `dq9-app` (natif, navigateur), options, commandes, événements, rendu et ses approximations |
| `docs/ressources-externes.md` | décompilation, wiki, portage PC, outils DS, licences |
| `docs/re/rom-layout.md` | structure de la ROM, overlays, systèmes de fichiers |
| [`docs/re/nitro-sdk.md`](docs/re/nitro-sdk.md) | routines NitroSDK ARM9 identifiées (classées Plateforme, non portées) |
| `docs/re/gpc2.md`, `table.md` | archives et compression Level-5, tables d'enregistrements |
| `docs/re/text.md`, `font.md` | textes, balises, pipeline des messages, polices |
| `docs/re/dialogue.md`, `trigger.md`, `stb.md` | répliques et fenêtre de message, triggers de scénario, scripts d'événements et leur exécution |
| `docs/re/stb-natives.md` | inventaire des 228 natives employées : arguments, effets établis, adresses et inconnus |
| `docs/re/maplist.md`, `map-archive.md`, `g3d.md` | liste des cartes, assemblage des cartes, portes et zones, modèles et animations G3D |
| `docs/re/characters.md` | PNJ, distribution par étape, sprites, modèles, héros |
| `docs/re/collision.md`, `movement.md` | collisions `.col2`, déplacement du héros et caméra |
| `docs/re/lighting.md` | éclairage `.bats`, brouillard, heure du jour |
| `docs/re/sdat.md` | son : SDAT, séquences, effets, jingles, musique des cartes |
| `docs/re/save.md` | sauvegarde DS : conteneur, sommes de contrôle, champs typés, inconnus |
| `docs/re/melonds-state.md` | oracle melonDS : lecture des sauvegardes d'état, adresses utiles, vérification restante |
| `docs/re/outils.md` | cache des requêtes Ghidra, adresse européenne vers source de la décompilation USA |
| `AGENTS.md` | contexte et règles pour les agents de développement |

## Légal

Ce projet ne contient ni ne distribue aucun fichier du jeu. Utilisez uniquement une copie que
vous possédez légalement. Code sous licence MIT ou Apache-2.0.
