# dq9-online

Réimplémentation en Rust de *Dragon Quest IX – Sentinels of the Starry Skies* (Nintendo DS),
destinée à tourner sur Windows, macOS, Linux et le Web. Le jeu est rétro-analysé à partir de
votre propre ROM ; aucune donnée du jeu n'est incluse dans ce dépôt.

## Arborescence

| Chemin | Rôle |
|---|---|
| `crates/nds-rom` | Lecture d'une ROM DS : en-tête, FNT/FAT, overlays, ARM9 (BLZ), LZ10/LZ11/Huffman/RLE, NARC. Sans E/S, compile en wasm. |
| `crates/dq9-formats` | Formats Level-5 de DQ9 : compression, archives GPC2 (`.gp2`), tables d'enregistrements, polices, pipeline des messages, triggers de scénario, choix des répliques des PNJ. Sans E/S. |
| `crates/dq9re` | CLI de rétro-ingénierie : `info`, `ls`, `stats`, `extract`, `gp2`, `table`, `text`, `font`, `msg`, `talk`, `narc`, `cat`. |
| `crates/testrom` | Aide aux tests : localise la ROM pour les tests sur données réelles. |
| `re/ghidra_scripts` | Scripts Ghidra : `NdsLayout` (carte mémoire), `ApplySymbols`, `Query`. |
| `re/symbols.txt` | Noms retrouvés (source de vérité versionnée, appliquée au projet Ghidra). |
| `scripts/` | Installation des outils, import Ghidra, requêtes et application des symboles. |
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
cargo build -p nds-rom -p dq9-formats --target wasm32-unknown-unknown
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
  `gp2 data/scenario/M02M0.gp2 --out build/x`, `narc data/map/F01M01.ambl -d --out build/narc`,
  `cat <fichier> -d --out x.bin`). `extract --unpack` décompresse toutes les archives `.gp2`
  dans `build/rom/gp2/` (pratique pour chercher avec `grep`).
- **Requêtes Ghidra scriptées** : `scripts/ghidra-query.ps1 decomp|xrefs|calls|disasm <adresse>`
  (adresses `0x020d91ec` ou `ov_017::0218c0f4`). `decomp` crée, sans l'enregistrer, la
  fonction que l'analyse n'a pas trouvée (code atteint seulement par une table de pointeurs).
- **`cargo dq9re talk data/scenario/M02M0.gp2 018_en.bin --quest 11=1 --level 25`** :
  applique les triggers de conversation de la carte, puis marque la réplique que le jeu
  choisit pour l'état donné (`--help` pour les options d'état).

### Consigner une découverte

1. Nommer les fonctions/données dans `re/symbols.txt` (`<adresse> <nom> # commentaire`), puis
   `scripts/ghidra-apply-symbols.ps1` (projet fermé dans l'interface). Le fichier est réappliqué
   à chaque `ghidra-import.ps1` : le projet Ghidra est jetable, les noms sont versionnés.
2. Décrire le format ou le comportement dans `docs/re/<sujet>.md` : preuves (adresses,
   contrôles sur données), faits observés et hypothèses marquées **[hypothèse]**.
3. Implémenter dans la crate correspondante avec un test sur données synthétiques et, si
   pertinent, un invariant vérifié sur la vraie ROM (`tests/real_rom.rs`).

Notes : `docs/re/rom-layout.md` (structure de la ROM), `docs/re/gpc2.md` (archives et
compression Level-5), `docs/re/table.md` (tables d'enregistrements), `docs/re/text.md`
(textes, balises, pipeline des messages), `docs/re/font.md` (polices et mise en page),
`docs/re/dialogue.md` (choix des répliques des PNJ, zones de conversation, `.npc`),
`docs/re/trigger.md` (triggers de scénario).

## Légal

Ce projet ne contient ni ne distribue aucun fichier du jeu. Utilisez uniquement une copie
que vous possédez légalement.
