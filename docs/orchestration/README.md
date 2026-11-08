# Orchestration — workflow du coordinateur

Documents : ce fichier (coordinateur), `regles-worker.md` (règles communes des workers),
`modele-lot.md` (spec d'un lot, copiée dans `build/lots/<lot>.md`), `prompt-dispatch.md`
(prompt de lancement d'un worker). État courant : `docs/status.md` uniquement.

## Harness et modèles

Toujours le harness **OMP** : workers Orca lancés avec OMP, sous-agents via l'outil `task`
d'OMP. Le modèle est choisi au lancement du worker et noté dans la spec du lot.

| Modèle | Id | Fournisseur | Usage |
|---|---|---|---|
| DeepSeek V4.1 Flash | `opencode-go/deepseek-v4.1-flash` | OpenCode Go | **défaut** : docs, scripts mécaniques, collecte, tests sur contrat donné |
| Sonnet 5.5 | `anthropic/claude-sonnet-5-5` | Anthropic | RE, contradicteur, conception, débogage non trivial, scripts touchant des fichiers utilisateur |
| Opus 5.5 | `anthropic/claude-opus-5-5` | Anthropic | dernier recours après échec documenté de Sonnet ; justification dans `docs/status.md` |

Escalade : Flash échoue deux fois ou dépasse son plafond → Sonnet ; Sonnet échoue → Opus,
seulement avec justification. Aucun autre modèle.

Agents OMP : `scout` n'a ni shell ni git (lecture de fichiers seulement) ; toute comparaison
de branches, `git cherry`/`range-diff` ou commande de build va à un agent `task`.

## Rôles

- **Coordinateur** : écrit les specs, lance les workers, intègre. Seul à écrire
  `docs/status.md`, `docs/roadmap.md`, `README.md`.
- **Worker** : un lot chacun, dans son worktree, selon `regles-worker.md`.
- **Contradicteur** : agent neuf (Sonnet). Reçoit l'affirmation et les moyens de la vérifier
  (état original, test, adresses, preuves), **pas** le raisonnement de l'auteur ; cherche à la
  réfuter. Obligatoire pour toute route, hauteur, transition ou cause de drapeau avant de
  l'écrire comme fait.

## Vague = DAG

RE → implémentation → contradicteur → intégration → lot final « cohérence des docs ».

- Lot « cohérence des docs » (Flash) : confronter `AGENTS.md`, `docs/architecture.md`,
  `README.md`, `docs/status.md` au code (grep des constantes comme la cadence, liste des
  crates…).
- Au plus 4-5 workers simultanés.
- Un propriétaire par fichier. Les fichiers partagés (`crates/dq9-app/src/app.rs`,
  `crates/dq9-app/src/event.rs`, `crates/dq9-core/src/field.rs`, `crates/dq9-core/src/state.rs`,
  `crates/dq9-core/src/trigger.rs`, `Cargo.toml`, `re/symbols.txt`) ne figurent dans le bloc
  ```owns que d'un seul lot à la fois.

## Cycle d'un lot

1. Écrire `build/lots/<lot>.md` depuis `modele-lot.md` (modèle, SHA de base, ```owns, plafond).
2. Dispatch Orca (worker OMP, modèle de la spec, branche `FabLrc/<jalon>-<lot>`) : prompt de
   `prompt-dispatch.md` rempli, lancé par la commande `orca orchestration worker-start` donnée
   dans ce fichier.
3. Attendre `worker_done` ; lire rapport et preuves.
4. Contrôles puis intégration depuis le checkout principal sur `main`, arbre propre :

   ```powershell
   ./scripts/integrate.ps1 -Branch FabLrc/j3-sound-effects -Spec build/lots/j3-sound-effects.md
   ./scripts/integrate.ps1 -Branch FabLrc/j3-sound-effects -Spec build/lots/j3-sound-effects.md -Apply -Message 'audio: play field sound effects'
   # libérer le worker côté Orca, puis :
   ./scripts/integrate.ps1 -Branch FabLrc/j3-sound-effects -Spec build/lots/j3-sound-effects.md -Cleanup
   ```

   Les contrôles vérifient : branche contenant `main` (sinon rebase), fichiers modifiés dans
   ```owns (sauf le bloc d'avancement du `README.md`, toujours accepté), aucun fichier interdit
   (`rom/`, `build/`, `tools/`, `*.nds`, `*.sav`), worktree
   propre, `verify.ps1 -RequireRom` vert et worktree encore propre après. `-Apply` fait un
   `git merge --squash` + un commit `domaine: résumé`, sans push. `-Cleanup` retire les
   jonctions sans suivre leur cible, puis le worktree et la branche.

   Ne jamais lancer `orca worktree rm` sur un worktree qui a encore des jonctions `rom`/`tools`
   (elles mènent à la seule copie de la ROM) : d'abord `integrate.ps1 -Cleanup`, qui retire les
   jonctions sans suivre leur cible. Un worktree sans jonction ni commit propre peut être retiré
   par `orca worktree rm --worktree path:<chemin> --delete-branch`.
5. Réécrire `docs/status.md` (< 60 lignes, réécrit, pas complété), commit, push.

## Scripts (interfaces du contrat)

| Script | Exemple |
|---|---|
| `scripts/verify.ps1 [-Fast] [-Only <step[,step]>] [-RequireRom]` | `./scripts/verify.ps1 -Only fmt,clippy` — étapes `fmt`, `clippy`, `test`, `wasm`, `coverage` ; seule source des commandes de vérification |
| `scripts/integrate.ps1 -Branch <b> -Spec <lot> [-Message <sujet>] [-Apply] [-Cleanup]` | voir cycle ci-dessus |
| `scripts/evidence.ps1 -Lot <lot> [-Command <texte>] [-Note <texte>]` | `./scripts/evidence.ps1 -Lot j3-sound-effects -Command 'cargo run -p dq9-app -- --headless' -Note 'capture'` → `build/evidence/<lot>/manifest.json` |
| `scripts/prune-build.ps1` | `./scripts/prune-build.ps1` — ne supprime jamais un fichier listé dans un manifeste de preuves |
| `scripts/prune-branches.ps1 [-Apply] [-Remote]` | `./scripts/prune-branches.ps1 -Remote` (liste), puis `-Apply` ; ne supprime jamais une branche unique |
| `scripts/parity.ps1 [-References <json>] [-Rom <path>]` | `./scripts/parity.ps1` — défaut `build/evidence/parity/references.json` ; non nul sur écart non accepté |

Les docs citent les preuves par `build/evidence/<lot>/<fichier>` + 12 premiers caractères du
sha256.

## Fin de vague

```powershell
./scripts/prune-branches.ps1          # relire la liste
./scripts/prune-branches.ps1 -Apply
./scripts/prune-build.ps1
```

Puis lancer le lot « cohérence des docs ».

## Plafonds

- Chaque lot a un plafond (durée ou budget) dans sa spec. Dépassement → un seul `ask` au
  coordinateur, puis arrêt ; pas de boucle de statut.
- Une RE ouverte (« poursuivre la référence ») est toujours découpée en objectifs vérifiables
  avant d'être confiée.

## CI

`.github/workflows/ci.yml` appelle `verify.ps1 -Only …`. Plus aucun commit du bot sur `main` :
le bloc d'avancement du `README.md` est mis à jour en local par `verify.ps1` (étape
`coverage`) et commité avec le lot ; le job coverage échoue si le bloc est périmé.
