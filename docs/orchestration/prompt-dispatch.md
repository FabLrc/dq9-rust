# Prompt de dispatch d'un worker

Le prompt est volontairement court : il renvoie aux fichiers versionnés (`regles-worker.md`,
spec du lot) au lieu de recopier les règles, pour ne jamais dériver d'eux. Remplacer les champs
`<lot>`, `<modèle>`, `<sha de base>`, `<chemin absolu de la spec>`, puis copier le bloc tel quel.

```text
Lot <lot> — harness OMP, modèle imposé : <modèle> (ne pas en changer ; sous-agents via `task`).

1. Lire d'abord docs/orchestration/regles-worker.md : il fait foi en cas de conflit.
2. Lire la spec du lot, en lecture seule, par chemin absolu (fichier ignoré par git, absent du
   worktree) : <chemin absolu de la spec>
   (ex. C:\Users\hiroc\Documents\ProjetsDev\dq9-online\build\lots\<lot>.md).
3. Lire AGENTS.md puis docs/status.md. Ne pas lire toute docs/roadmap.md.

Règles :
- Partir de <sha de base>. Ne rebaser sur main que si le coordinateur le demande.
- Ne modifier que les fichiers couverts par le bloc ```owns de la spec. Jamais rom/, tools/,
  build/ (hors preuves), *.nds, *.sav, docs/status.md, docs/roadmap.md, README.md.
- Pendant le travail : ./scripts/verify.ps1 -Fast. Une fois à la fin :
  ./scripts/verify.ps1 -RequireRom (doit passer ; commiter le bloc d'avancement du README.md
  s'il a été régénéré).
- Preuves sous build/evidence/<lot>/, puis
  ./scripts/evidence.ps1 -Lot <lot> -Command '<commande>' -Note '<contexte>'.
- Commits sur la branche du worktree uniquement ; ne pas pousser, ne pas fusionner.
- Bloqué ou plafond atteint : un seul `ask` au coordinateur, puis arrêt.
- Terminer par `worker_done` au format de regles-worker.md § 5.
```

Lancement (depuis le checkout principal) :

```powershell
orca orchestration worker-start --spec "<prompt rempli>" --worktree new-top-level `
  --repo id:8ef45a12-a7ec-4e6c-b9dc-c85007817f63 --name <lot> --model <modèle>
```
