# dq9-online

Réimplémentation en Rust de *Dragon Quest IX – Sentinels of the Starry Skies* (Nintendo DS),
pour Windows, macOS, Linux et le Web, avec le multijoueur de la DS rendu possible en ligne.
Le jeu est rétro-analysé et ses ressources sont lues depuis **votre propre ROM** ; aucune
donnée du jeu n'est incluse dans ce dépôt.

**État** : cartes texturées avec PNJ et animations en natif/Web, déplacement de terrain
expérimental (`dq9-core`) ; portes, progression interactive et scènes restent à faire. Prochain
objectif : une **v0.1 jouable** (prologue d'Angel Falls). Plan : `docs/roadmap.md`, lancement :
`docs/app.md`.

<!-- coverage:start -->
**Avancement : 2,0 %** des fonctions de jeu traduites (251 / 12 056), 2,5 % du code (65 Ko / 2 505 Ko).
Calculé par `cargo dq9re coverage` à partir des marqueurs `port:` du code (définition dans `docs/roadmap.md`).

<details><summary>Détail par module</summary>

| Module | Fonctions traduites | Code traduit |
|---|---:|---:|
| ARM9 | 222 / 5 464 (4,0 %) | 56 Ko / 826 Ko (6,7 %) |
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
| `crates/nds-rom` | ROM DS : en-tête, FNT/FAT, overlays, ARM9 (BLZ), LZ10/LZ11/Huffman/RLE, NARC. Sans E/S, compile en wasm. |
| `crates/dq9-formats` | Formats Level-5 de DQ9 : compression, archives `.gp2`, tables d'enregistrements, polices, messages, choix des répliques, triggers, scripts `.stb`. Sans E/S, compile en wasm. |
| `crates/dq9-render` | Rendu wgpu des cartes, proche du moteur 3D de la DS ; caméra libre. Sans E/S, compile en wasm. |
| `crates/dq9-core` | État déterministe, requêtes de collision et déplacement provisoire. Sans E/S, compile en wasm. |
| `crates/dq9-app` | Application (winit + wgpu, natif et Web) : `cargo run -p dq9-app --release -- --map M01` (`docs/app.md`). |
| `crates/dq9re` | CLI de rétro-ingénierie (`cargo dq9re --help`). |
| `crates/testrom` | Localise la ROM pour les tests sur données réelles. |
| `re/symbols.txt` | Noms retrouvés, source de vérité appliquée au projet Ghidra. |
| `re/ghidra_scripts/`, `scripts/` | Scripts Ghidra et PowerShell : installation des outils, import, requêtes, symboles. |
| `docs/` | Feuille de route, architecture, ressources externes, notes de RE (`re/`). |
| `rom/` | Votre ROM (ignorée par git). |

`dq9-audio` sera créée au fil de la feuille de route (`docs/architecture.md`).

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
cargo test --workspace                        # tests sur la vraie ROM inclus si elle est présente
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo build -p nds-rom -p dq9-formats -p dq9-core -p dq9-render -p dq9-app --target wasm32-unknown-unknown
cargo dq9re coverage --readme README.md       # avancement (sans ROM), réécrit le bloc ci-dessus
```

La CI exécute fmt, clippy, les tests sous Linux et la compilation wasm, sans jamais avoir accès
à la ROM, puis calcule l'avancement et met à jour ce README sur `main`.

## Rétro-ingénierie

- **`cargo dq9re`** : `ls data/map/`, `gp2 <archive> --out <dossier>`, `table <archive>
  <membre>`, `msg`, `font`, `narc <fichier> -d`, `cat <fichier> -d`,
  `talk data/scenario/M02M0.gp2 018_en.bin --quest 11=1 --level 25` (triggers puis réplique
  choisie pour un état donné), `stb data/event/ev03030.gp2` (script d'événement désassemblé
  avec ses textes).
- **Ghidra** : ARM9 dans le bloc `arm9`, ITCM/DTCM à leurs adresses d'exécution, chaque
  overlay dans son espace `ov_NNN`. Requêtes sans interface :
  `scripts/ghidra-query.ps1 decomp|xrefs|calls|disasm <adresse>` (`0x020d91ec` ou
  `ov_017::0218c0f4`) ; une requête verrouille le projet, en lancer plusieurs demande des
  copies de `re/ghidra` désignées par `DQ9_GHIDRA_PROJECT`.
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

## Documentation

| Fichier | Contenu |
|---|---|
| `docs/roadmap.md` | jalons jusqu'au portage complet, définition du « 100 % » |
| `docs/architecture.md` | crates cibles et règles (portabilité, déterminisme, traçabilité) |
| `docs/app.md` | lancer `dq9-app` (natif, navigateur), rendu et ses approximations |
| `docs/ressources-externes.md` | décompilation, wiki, portage PC, outils DS, licences |
| `docs/re/rom-layout.md` | structure de la ROM, overlays, systèmes de fichiers |
| [`docs/re/nitro-sdk.md`](docs/re/nitro-sdk.md) | premier lot de 50 routines NitroSDK ARM9 identifiées, non portées |
| `docs/re/gpc2.md`, `table.md` | archives et compression Level-5, tables d'enregistrements |
| `docs/re/text.md`, `font.md` | textes, balises, pipeline des messages, polices |
| `docs/re/dialogue.md`, `trigger.md`, `stb.md` | répliques des PNJ, triggers de scénario, scripts d'événements |
| `docs/re/stb-natives.md` | inventaire des 228 natives employées : arguments, effets établis, adresses et inconnus |
| `docs/re/save.md` | sauvegarde DS : conteneur, sommes de contrôle, champs typés, inconnus |
| `docs/re/melonds-state.md` | oracle melonDS : lecture des sauvegardes d'état, adresses utiles, vérification restante |
| `AGENTS.md` | contexte et règles pour les agents de développement |

## Légal

Ce projet ne contient ni ne distribue aucun fichier du jeu. Utilisez uniquement une copie que
vous possédez légalement. Code sous licence MIT ou Apache-2.0.
