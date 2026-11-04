# dq9-online

Réimplémentation en Rust de *Dragon Quest IX – Sentinels of the Starry Skies* (Nintendo DS),
destinée à tourner sur Windows, macOS, Linux et le Web. Le jeu est rétro-analysé à partir de
votre propre ROM ; aucune donnée du jeu n'est incluse dans ce dépôt.

## Arborescence

| Chemin | Rôle |
|---|---|
| `crates/nds-rom` | Lecture d'une ROM DS : en-tête, FNT/FAT, overlays, ARM9 (BLZ), LZ10/LZ11, NARC. Sans E/S, compile en wasm. |
| `crates/dq9re` | CLI de rétro-ingénierie : `info`, `ls`, `stats`, `extract`, `narc`, `cat`. |
| `re/ghidra_scripts` | Scripts Ghidra (`NdsLayout.java` : carte mémoire ARM9 + overlays). |
| `scripts/` | Installation des outils, import Ghidra, lancement de Ghidra. |
| `docs/` | Architecture (`architecture.md`) et notes de RE (`re/`). |
| `rom/` | Emplacement de votre ROM (ignoré par git). |
| `build/`, `tools/`, `re/ghidra/` | Générés, ignorés par git. |

## Mise en place (Windows)

Prérequis installés sur cette machine : Rust (rustup, hôte `x86_64-pc-windows-gnu`, cible
`wasm32-unknown-unknown`) et MinGW-w64 WinLibs (fournit `dlltool`/`as` exigés par la cible GNU),
tous deux ajoutés au `PATH` utilisateur — ouvrez un nouveau terminal après installation.
La version de Rust est épinglée par `rust-toolchain.toml`.

```powershell
# 1. Outils RE dans ./tools (JDK 25, Ghidra 12.2, melonDS 1.1 ; SHA-256 vérifiés)
powershell -ExecutionPolicy Bypass -File scripts/setup-re-tools.ps1

# 2. Vérifier la ROM et extraire code + données dans build/rom
cargo dq9re info
cargo dq9re extract              # --code-only pour ne sortir que les binaires

# 3. Projet Ghidra re/ghidra/dq9.gpr (analyse automatique, ~3 min)
powershell -ExecutionPolicy Bypass -File scripts/ghidra-import.ps1
powershell -ExecutionPolicy Bypass -File scripts/ghidra.ps1   # interface graphique
```

La ROM est cherchée dans `rom/`, puis à la racine ; sinon `--rom <chemin>` ou `DQ9_ROM`.

## Développement

```powershell
cargo test --workspace                       # inclut les tests sur la vraie ROM si présente
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo build -p nds-rom --target wasm32-unknown-unknown
```

La CI (`.github/workflows/ci.yml`) exécute fmt, clippy, les tests sur Linux/Windows/macOS et la
compilation wasm. Elle n'a jamais accès à la ROM.

## Outils de rétro-ingénierie

- **Ghidra** : analyse statique. Le module ARM9 est dans le bloc `arm9`, l'ITCM/DTCM à leurs
  adresses d'exécution, chaque overlay dans son propre espace `ov_NNN` (les overlays d'un même
  emplacement se chevauchent en RAM).
- **melonDS** (`tools/melonds/melonDS.exe`) : référence visuelle et analyse dynamique via son
  stub GDB (à activer dans les paramètres de l'émulateur).
- **`cargo dq9re`** : exploration rapide des fichiers (`ls data/map/`,
  `narc data/map/F01M01.ambl --lz --out build/narc`, `cat <fichier> --lz --out x.bin`).

Les constatations sont consignées dans `docs/re/` (voir `rom-layout.md`), en séparant faits
observés et hypothèses.

## Légal

Ce projet ne contient ni ne distribue aucun fichier du jeu. Utilisez uniquement une copie
que vous possédez légalement.
