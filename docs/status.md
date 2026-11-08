# État du projet

Réécrit par le coordinateur à chaque intégration ; ne pas y ajouter de journal.
Plan et jalons : [`roadmap.md`](roadmap.md) ; règles de travail : [`../AGENTS.md`](../AGENTS.md).

## État au 8 novembre 2026 (`main` = `5b32cf1`)

- **v0.1 fonctionnelle en natif** : `cargo run -p dq9-app --release -- --new-game` joue
  l'ouverture, le prologue de l'Observatoire, le retour physique à Angel Falls (1100) et les
  scènes du village ; tâches 1.2 (anneau, veuve, écurie), verrou de sortie, aire 12 et retour
  1.4.1 couverts par des tests réels (`keepsake_ring_task_…`, `widow_task_…`,
  `stable_pickups_…`, `house_exit_requires_both_task_flags_…`,
  `village_area_12_plays_1410_…`). Combats substitués (« gagné »).
- **Cadence** : boucle fixe à 60 Hz ; héros, `Field::step` et pose du modèle à 30 Hz
  (images impaires, `Hero::step(ticks = 2)`, `7db7f41`, test
  `field_cadence_holds_the_pose_and_matches_the_original_every_vblank`).
- **Parité mesurée** : premier contrôle et point de contrôle libre 1.1.2 identiques à
  l'original sauf `4135`, butin aléatoire du combat 26 (écart accepté, `re/item.md`).
- **Navigateur** (Pages, build `21e1e42`) : ouverture, premier contrôle, 1140, 1130 et hall
  surélevé ; pas de session continue jusqu'à 4107, la tour ou Angel Falls (`app.md`,
  § Vérification).

## Lots actifs

Aucun. Les branches `FabLrc/v01-*7` sont intégrées (`git cherry main <branche>`). Deux
gardent des commits non équivalents par patch : `v01-return7` (versions antérieures de
`re/movement.md`, remplacées dans `main`) et `v01-web7` (`d13c8fb`, durée de franchissement
de porte, reportée dans `app.md`). `v01-init-flags` et `v01-height` sont historiques :
conclusions reprises dans `main`, ne pas réintégrer. Ces branches uniques restent jusqu'à
décision de l'utilisateur (`scripts/prune-branches.ps1` ne les supprime jamais).

## Blocages et prochaines étapes

1. **Référence originale au-delà de `X01M09`** : le retour bas `X01M01` → `X01M05` n'est
   pas obtenu dans l'original (**[inconnu]**) et le rejeu direct des pads natifs est
   falsifié (`re/melonds-state.md`, § Portes du retour). Il faut des positions ou points de
   passage monde, pas des pads.
2. **Parité au-delà de 1.1.2** : produire avec le harnais les références d'Angel Falls,
   des tâches du village et de la fin du prologue, puis les rejouer par
   `scripts/parity.ps1`.
3. **Cadence des portes** : `Field::step` à 30 Hz (événement 17 en 8 VBlanks) reste
   **[hypothèse]** ; mesure melonDS 4 contre 8 VBlanks à faire (`re/movement.md`).
4. **Certification navigateur de bout en bout** : session continue sur Pages jusqu'à
   Angel Falls et les tâches du village, à la main (le pilote n'est pas un critère produit).
5. **J4, combat** : le butin du combat 26 doit sortir de sa propre règle pour lever `4135`.

## Preuves

Locales, jamais commitées. Nouveaux lots : `build/evidence/<lot>/manifest.json`
(`scripts/evidence.ps1`). Anciennes racines : `build/v01-20261107/` et `build/orca/` ;
les notes `docs/re/*.md` et `app.md` citent chemins et empreintes.

## Comparaison à l'original

`scripts/parity.ps1` rejoue les références (`build/evidence/parity/references.json`) sur
le runtime headless ; écarts acceptés listés dans la référence avec leur justification.
`scripts/compare-runs.ps1` et `dq9re state diff/watch` servent au diagnostic.
