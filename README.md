# dq9-online

Réimplémentation en Rust de *Dragon Quest IX – Sentinels of the Starry Skies* (Nintendo DS),
pour Windows, macOS, Linux et le Web, avec le multijoueur de la DS rendu possible en ligne.
Le jeu est rétro-analysé et ses ressources sont lues depuis **votre propre ROM** ; aucune
donnée du jeu n'est incluse dans ce dépôt.

**État** : formats et logique de dialogue/scénario réimplémentés (archives, textes, polices,
choix des répliques, triggers, scripts d'événements) ; pas encore de jeu affiché. Prochain
objectif : une **v0.1 jouable** (prologue d'Angel Falls). Plan complet : `docs/roadmap.md`.

## Arborescence

| Chemin | Rôle |
|---|---|
| `crates/nds-rom` | ROM DS : en-tête, FNT/FAT, overlays, ARM9 (BLZ), LZ10/LZ11/Huffman/RLE, NARC. Sans E/S, compile en wasm. |
| `crates/dq9-formats` | Formats Level-5 de DQ9 : compression, archives `.gp2`, tables d'enregistrements, polices, messages, choix des répliques, triggers, scripts `.stb`. Sans E/S, compile en wasm. |
| `crates/dq9re` | CLI de rétro-ingénierie (`cargo dq9re --help`). |
| `crates/testrom` | Localise la ROM pour les tests sur données réelles. |
| `re/symbols.txt` | Noms retrouvés, source de vérité appliquée au projet Ghidra. |
| `re/ghidra_scripts/`, `scripts/` | Scripts Ghidra et PowerShell : installation des outils, import, requêtes, symboles. |
| `docs/` | Feuille de route, architecture, ressources externes, notes de RE (`re/`). |
| `rom/` | Votre ROM (ignorée par git). |

Les crates `dq9-core`, `dq9-render`, `dq9-audio` et `dq9-app` seront créées au fil de la
feuille de route (`docs/architecture.md`).

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
```

## Développement

```powershell
cargo test --workspace                        # tests sur la vraie ROM inclus si elle est présente
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo build -p nds-rom -p dq9-formats --target wasm32-unknown-unknown
```

La CI exécute fmt, clippy, les tests sous Linux et la compilation wasm, sans jamais avoir accès
à la ROM.

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
- **melonDS** (`tools/melonds`) : référence visuelle et analyse dynamique par son stub GDB.
- **Avant d'analyser** : symboles et code de la [décompilation USA](https://github.com/DQIX/dqix-decomp)
  (adresses à convertir pour la ROM européenne) et [wiki des formats DQIX](https://github.com/DQIX/wiki),
  voir `docs/ressources-externes.md`.

Consigner une découverte :

1. Nommer fonctions et données dans `re/symbols.txt` (`<adresse> <nom> # commentaire`), puis
   `scripts/ghidra-apply-symbols.ps1` (projet fermé dans l'interface). Le projet Ghidra est
   jetable, les noms sont versionnés.
2. Décrire le format ou le comportement dans `docs/re/<sujet>.md` : preuves (adresses,
   contrôles sur données), hypothèses marquées **[hypothèse]**.
3. Implémenter dans la crate correspondante avec un test sur données synthétiques et, si
   pertinent, un invariant sur la vraie ROM (`tests/real_rom.rs`).

## Documentation

| Fichier | Contenu |
|---|---|
| `docs/roadmap.md` | jalons jusqu'au portage complet, définition du « 100 % » |
| `docs/architecture.md` | crates cibles et règles (portabilité, déterminisme, traçabilité) |
| `docs/ressources-externes.md` | décompilation, wiki, portage PC, outils DS, licences |
| `docs/re/rom-layout.md` | structure de la ROM, overlays, systèmes de fichiers |
| `docs/re/gpc2.md`, `table.md` | archives et compression Level-5, tables d'enregistrements |
| `docs/re/text.md`, `font.md` | textes, balises, pipeline des messages, polices |
| `docs/re/dialogue.md`, `trigger.md`, `stb.md` | répliques des PNJ, triggers de scénario, scripts d'événements |
| `AGENTS.md` | contexte et règles pour les agents de développement |

## Légal

Ce projet ne contient ni ne distribue aucun fichier du jeu. Utilisez uniquement une copie que
vous possédez légalement. Code sous licence MIT ou Apache-2.0.
