# Modèle de lot

Copier ce fichier dans `build/lots/<lot>.md` (local, ignoré par git) et remplir chaque champ.
La spec vit dans le checkout principal, pas dans le worktree du worker : celui-ci la lit en
lecture seule par chemin absolu (`C:\Users\hiroc\Documents\ProjetsDev\dq9-online\build\lots\<lot>.md`,
voir `prompt-dispatch.md`). La spec est la seule entrée du worker avec `docs/orchestration/regles-worker.md`. Le bloc
```owns est lu par `scripts/integrate.ps1` : tout fichier modifié hors de ces globs bloque
l'intégration.

---

## Lot

`<jalon>-<lot>` — branche `FabLrc/<jalon>-<lot>` (ex. `j3-sound-effects`).

## Modèle

Un id exact parmi :

- `opencode-go/deepseek-v4.1-flash` (par défaut : mécanique, docs, scripts, tests sur contrat) ;
- `anthropic/claude-sonnet-5-5` (RE, contradicteur, conception, code à risque) ;
- `anthropic/claude-opus-5-5` (dernier recours après échec documenté de Sonnet ; justification
  dans `docs/status.md`).

Raison du choix : `<une phrase>`.

## SHA de base

`<sha de main au lancement>` — le worker part de ce commit ; `integrate.ps1` exige que la
branche contienne `main` (sinon rebase).

## Objectif

`<résultat attendu en une ou deux phrases, vérifiable>`

## Existant

À lire avant de commencer (en plus de `AGENTS.md` et `docs/status.md`) :

- `docs/re/<sujet>.md`
- `crates/<crate>/src/<module>.rs`

## Changement

1. `<étape>`
2. `<étape>`

## Périmètre

Globs relatifs à la racine, `/` comme séparateur ; `**` = n'importe quel chemin, `*` = tout
sauf `/`, `?` = un caractère ; lignes vides et `#` ignorées.

```owns
# exemple
crates/dq9-core/src/sound/**
crates/dq9-core/tests/sound_*.rs
docs/re/sound.md
# fichier partagé : annoncé dans un seul lot à la fois
crates/dq9-app/src/app.rs
```

Fichiers interdits (toujours) : `rom/`, `build/`, `tools/`, `*.nds`, `*.sav`, ainsi que
`docs/status.md`, `docs/roadmap.md`, `README.md`. Seule exception : le bloc d'avancement du
`README.md` (entre `<!-- coverage:start -->` et `<!-- coverage:end -->`) régénéré par
`verify.ps1`, que `integrate.ps1` accepte sans qu'il figure dans ```owns. Autres interdits
propres au lot : `<liste>`.

## Acceptation

Comportement observable : `<ce que l'on voit / mesure>`.

Commandes (toutes doivent passer) :

```powershell
./scripts/verify.ps1 -RequireRom
<smoke propre au lot, ex. cargo run -p dq9-app -- --headless ...>
./scripts/parity.ps1          # seulement si le lot touche le jeu
```

## Preuves

Fichiers sous `build/evidence/<lot>/`, puis :

```powershell
./scripts/evidence.ps1 -Lot <lot> -Command '<commande qui a produit les preuves>' -Note '<contexte>'
```

Citer dans la note et le rapport `build/evidence/<lot>/<fichier>` + 12 premiers caractères de
son sha256 (lu dans le manifeste).

## Plafond

`<durée ou budget, ex. 2 h / 40 tours>`. Au-delà : un seul `ask` au coordinateur, puis arrêt.

## Conditions d'`ask`

- décision qui touche le périmètre d'un autre lot ou une API publique utilisée ailleurs ;
- besoin d'un fichier hors du bloc ```owns ;
- plafond atteint ;
- `<autre condition propre au lot>`.

## Contradicteur requis

`oui` / `non`. Si oui, affirmation à réfuter : `<ex. « la transition X est déclenchée par le
drapeau Y »>` et moyens de vérification fournis (état original, test, adresses).
