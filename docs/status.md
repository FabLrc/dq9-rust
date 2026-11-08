# État du projet

Réécrit par le coordinateur à chaque intégration ; ne pas y ajouter de journal.
Plan et jalons : [`roadmap.md`](roadmap.md) ; règles de travail : [`../AGENTS.md`](../AGENTS.md).

## État au 8 novembre 2026 (`main` après la reconstruction du cast 0x8d)

- **v0.1 fonctionnelle en natif** : `cargo run -p dq9-app --release -- --new-game` joue
  l'ouverture, le prologue de l'Observatoire, le retour physique à Angel Falls (1100) et les
  scènes du village ; tâches 1.2 (anneau, veuve, écurie), verrou de sortie, aire 12 et retour
  1.4.1 couverts par des tests réels (`keepsake_ring_task_…`, `widow_task_…`,
  `stable_pickups_…`, `house_exit_requires_both_task_flags_…`,
  `village_area_12_plays_1410_…`). Combats substitués (« gagné »).
- **Progression sur place** : les actions différées rechargent les triggers (événement 20) et
  0x8d reconstruit le cast de la carte sans la recharger (`re/trigger.md`, § Rechargements) ;
  le pilote ne recharge plus la carte, et son rejeu redonne ses 615 points de contrôle octet
  pour octet jusqu'à `M01M08`.
- **Cadence** : boucle fixe à 60 Hz ; héros, `Field::step` et pose du modèle à 30 Hz
  (images impaires, `Hero::step(ticks = 2)`, `7db7f41`, test
  `field_cadence_holds_the_pose_and_matches_the_original_every_vblank`).
- **Parité mesurée** : premier contrôle et point de contrôle libre 1.1.2 identiques à
  l'original sauf `4135`, butin aléatoire du combat 26 (écart accepté, `re/item.md`).
- **Navigateur** (Pages, build `21e1e42`) : ouverture, premier contrôle, 1140, 1130 et hall
  surélevé ; pas de session continue jusqu'à 4107, la tour ou Angel Falls (`app.md`,
  § Vérification).

## Lots actifs

Aucun. Seule `main` reste, en local comme sur `origin` : les branches intégrées ont été
supprimées, `v01-init-flags` après report de `Party_FlagEquippedItems` (`re/item.md`).

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
6. **Reconstruction du cast** : le fondu de la tâche 0x30 (~30 images) n'est pas modélisé ;
   ce que `021a2fa0` reconstruit en plus du cast (monstres, objets) est **[inconnu]** ; aucun
   état original à 4104 en 1.2 ne confirme encore le PNJ 99 sans changement de carte.

## Preuves

Locales, jamais commitées. Nouveaux lots : `build/evidence/<lot>/manifest.json`
(`scripts/evidence.ps1`). Anciennes racines : `build/v01-20261107/` et `build/orca/` ;
les notes `docs/re/*.md` et `app.md` citent chemins et empreintes.

## Comparaison à l'original

`scripts/parity.ps1` rejoue les références (`build/evidence/parity/references.json`) sur
le runtime headless ; écarts acceptés listés dans la référence avec leur justification.
`scripts/compare-runs.ps1` et `dq9re state diff/watch` servent au diagnostic.
