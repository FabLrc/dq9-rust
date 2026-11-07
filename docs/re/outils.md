# Outils : cache Ghidra et source USA

Deux scripts accélèrent la rétro-ingénierie : `scripts/ghidra-query.ps1` met les réponses de
Ghidra en cache et `scripts/decomp.ps1` relie directement une adresse ou un nom européens au
code source de la décompilation américaine. Tous deux fonctionnent depuis un worktree lié :
les outils, le projet Ghidra et la ROM sont alors lus dans le dépôt principal quand le
worktree ne les contient pas (ils sont ignorés par git).

## `scripts/decomp.ps1` — adresse EU vers source USA

La décompilation [DQIX/dqix-decomp](https://github.com/DQIX/dqix-decomp) est sous licence
Unlicense, mais son checkout n'est pas commité : il est attendu dans `tools/dqix-decomp` (ou
désigné par `-Decomp`). `re/functions.txt` fournit les fonctions européennes (adresse,
taille, catégorie, nom de la décompilation) ; le script retrouve le symbole correspondant
dans `config/usa/arm9/**/symbols.txt`, puis le fichier source dans le `delinks.txt` du
module. Pour l'ARM9, l'adresse USA est obtenue par la conversion inverse de
`usa_to_eur` (`crates/dq9re/src/coverage.rs`) : `-8` sur `0x0200F3C0..0x0200FB30`, `-0x10`
sur `0x0200FB30..0x020EEBE0`, inchangée ailleurs. Overlays et ITCM ont les mêmes adresses
dans les deux versions.

```powershell
powershell -ExecutionPolicy Bypass -File scripts/decomp.ps1 0x02010250
powershell -ExecutionPolicy Bypass -File scripts/decomp.ps1 GameState::SetDayTimer
powershell -ExecutionPolicy Bypass -File scripts/decomp.ps1 ov_017::0218b5a0 -Context 20
powershell -ExecutionPolicy Bypass -File scripts/decomp.ps1 -List SetDayTimer
powershell -ExecutionPolicy Bypass -File scripts/decomp.ps1 0x02010250 -Decomp D:\dqix-decomp
```

- Une adresse est cherchée comme dans Ghidra : plus grande adresse de début inférieure ou
  égale, tant qu'elle précède le début suivant. Une adresse d'overlay exige le préfixe
  `ov_NNN::` ; une adresse ARM9 qui tombe après la dernière fonction le rappelle.
- Un nom est cherché d'abord à l'identique (symbole brut, nom démanglé, puis dernière
  composante), ensuite par sous-chaîne. Plusieurs résultats donnent la liste des candidats et
  le code de sortie 2 ; `-List` affiche les adresses et les noms sans extrait.
- `-Context N` affiche N lignes à partir de la définition (40 par défaut). Les symboles non
  démanglés sont rendus par un démangleur Itanium minimal (`_ZN...E`, constructeurs,
  destructeurs) ; un nom inconnu est affiché tel quel.
- Si le checkout, le symbole ou le source manquent, le script explique lequel et sort en
  erreur (code 1) : il n'invente pas de chemin. Beaucoup de fonctions d'overlays n'ont pas
  encore de source dans la décompilation, seulement un symbole.

Exemples vérifiés le 6 novembre 2026 sur le checkout `b5c617a` :

```text
$ scripts/decomp.ps1 0x02010250
eu    0x02010250  0x40  game  GameState::AdvanceDayTimer
usa   0x02010240  _ZN9GameState15AdvanceDayTimerEv
src   ...\tools\dqix-decomp\src\GameState\GameTime.cpp:28
    28: void GameState::AdvanceDayTimer()

$ scripts/decomp.ps1 ov_017::0218b5a0 -Context 4
eu    ov_017::0218b5a0  0x10  game  SetGameResources
usa   0x0218b5a0  _Z16SetGameResourcesP13GameResources
src   ...\src\Resource\GameResources.cpp:16
    16: void SetGameResources(GameResources* gameResources)
```

`0x02010250` est bien `AdvanceDayTimer` aux deux versions (USA `0x02010240`) ;
`GameState::SetDayTimer` commence à l'adresse européenne `0x02010298` (USA
`0x02010288`) et se retrouve par son nom. Le cas ITCM est couvert par le même chemin :
`0x01ff8810` donne `VRAMStagingManager::ZeroInitialize` dans
`src/Graphics/itcm/VRAMStagingITCM.cpp`.

## `scripts/ghidra-query.ps1` — cache des requêtes Ghidra

Une requête `analyzeHeadless` coûte 5 à 30 s. Le résultat filtré est maintenant écrit dans
`build/ghidra-cache/<clé>.txt` et relu tel quel au prochain appel identique. La clé SHA-256
couvre :

- la commande et ses arguments ;
- l'identité de la ROM : chemin canonique, taille et date de modification (`DQ9_ROM`, sinon
  `rom/*.nds` puis la racine, dans le worktree ou le dépôt principal) ;
- le SHA-256 de `re/ghidra_scripts/Query.java`.

`cache: hit` ou `cache: miss` est écrit sur la sortie d'erreur ; la sortie standard reste
celle de `Query.java`. `-Refresh` ignore l'entrée existante et la réécrit. Un échec
d'`analyzeHeadless`, ou une sortie vide (arguments invalides, projet verrouillé), n'est pas
mis en cache.

```powershell
powershell -ExecutionPolicy Bypass -File scripts/ghidra-query.ps1 decomp 0x02010250
powershell -ExecutionPolicy Bypass -File scripts/ghidra-query.ps1 decomp 0x02010250   # cache: hit
powershell -ExecutionPolicy Bypass -File scripts/ghidra-query.ps1 xrefs 0x02010250 -Refresh
powershell -ExecutionPolicy Bypass -File scripts/ghidra-query.ps1 calls 0x02010250 0x02010298
```

Mesure du 6 novembre 2026 sur la ROM et le projet locaux, deux appels identiques à
`decomp 0x02010250` : `cache: miss` en 5 193 ms, puis `cache: hit` en 289 ms, sorties
identiques. Les autres formes (`xrefs`, `calls` avec plusieurs adresses) ont été exécutées
avec succès avant et après la mise en cache.

Limites :

- `re/symbols.txt` ou l'analyse du projet peuvent changer sans changer la clé : après avoir
  appliqué des symboles, `-Refresh` est nécessaire. `Query.java` est versionné par son
  contenu, pas le projet Ghidra.
- Le cache est propre au worktree appelant (`build/` est ignoré) ; il n'est jamais commité et
  ne contient que la sortie de Ghidra. Les extraits affichés restent des données du jeu : ne
  pas les recopier dans le dépôt au-delà de ce qu'exige une preuve.
- Une requête verrouille toujours le projet pendant son exécution ; pour du parallélisme,
  garder une copie de `re/ghidra` par requête et `DQ9_GHIDRA_PROJECT`, le cache n'y change
  rien.

## Traçage mémoire : `DQIX_WATCH` de DQIX Enhanced

[DQIX Enhanced](https://github.com/sackchief/dqix-enhanced) (GPL-3.0, v33, commit
`86ff9b1`) est une recompilation statique du code européen. C'est un **outil externe** :
on peut l'exécuter pour observer le jeu, jamais copier ni lier son code dans les crates
Rust. Il n'a **aucun binaire prébuild** ; le build depuis la ROM est obligatoire (MSYS2
UCRT64, 10 à 20 min la première fois, ~3 Go) :

```sh
./tools/build-game.sh /c/chemin/YDQP.nds   # game/dqix.exe, game/data/code, game/dqixcore.dll
```

Son mode headless ne crée pas de fenêtre et journalise les écritures ARM9 sur `stderr`,
ce qui en fait le traceur mémoire des drapeaux dont l'écrivain n'est pas encore connu :

```powershell
$env:DQIX_NOWINDOW   = "1"
$env:DQIX_NO_SETTINGS = "1"
$env:SDL_AUDIODRIVER = "dummy"
$env:DQIX_RTC        = "1760000000"       # horloge fixe, reproductible
$env:DQIX_INPUT_FILE = "build/watch/inputs.txt"
$env:DQIX_RAMDUMP    = "build/watch/ram,12000,12100"
$env:DQIX_WATCH      = "02108ADC"         # un seul mot ARM9, aligné sur 4 octets
$env:DQIX_WATCHREGS  = "1"
$env:DQIX_TRACEN     = "64"
.\game\dqix.exe <ROM> game\data\code 12201 1 build\watch\eval.sav 2> build\watch\watch.log
```

Chaque écriture produit `[f<frame> l<vblank>] watch: write32 <adresse> = <valeur> (lr
<lr>)`, suivi des `r0..r12` avec `DQIX_WATCHREGS`, puis des dernières fonctions ARM9
entrées (noms si `-DRECOMP_TRACE`). `DQIX_RAMDUMP` écrit des images brutes de 4 Mio aux
images demandées, lisibles par `dq9re state --ram` une fois ce mode livré. Limites : un
seul mot surveillé par exécution, pas de PC ni de pile complète, pas de valeur avant
l'écriture. Le raisonnement complet et les chemins de code (v33, `runtime/mem.c` l.
157-188, `runtime/io.c` l. 8-19) sont dans le rapport local
`build/eval-dqix-enhanced/RAPPORT-watch-drapeaux.md` ; au 6 novembre 2026, les unités
`mem.c`, `dispatch.c` et `frontend_none.c` compilent avec MinGW-W64, mais le link complet
avec la ROM reste à exécuter.

## Comparer deux runs : `scripts/compare-runs.ps1`

Le runtime (`dq9-app --checkpoints`) et le harnais libretro écrivent le même JSONL : un
objet par point, avec `map`, `track`, `tracks` (chapitre, étape, sous-étape et leurs
drapeaux), `flags`, `quests`, `daytime`, `hero` et un `kind` libre. Les deux runs n'ont pas
la même base de temps : melonDS avance à l'horloge réelle et le runtime compresse les
attentes. Le script aligne donc par **point d'histoire** (`carte:chapitre.étape.sous-étape`)
et par `kind`, pas par numéro d'image.

```powershell
powershell -ExecutionPolicy Bypass -File scripts/compare-runs.ps1 `
    -Original build/oracle-host/checkpoints.jsonl `
    -Runtime  build/oracle/runtime.jsonl -Limit 100
```

Règles d'alignement (corrigées le 7 novembre 2026 ; le lot est en validation, le script de
`main` avant ce lot réutilisait les points runtime et sortait 0 sur zéro alignement) :

- un checkpoint runtime ne sert **qu'une fois** : deux checkpoints originaux de même clé ne
  peuvent pas être « prouvés » par le même point runtime ;
- même `kind` d'abord ; seule exception, un `after` original (relevé après la prise de
  contrôle) peut s'apparier au `control` runtime ;
- un `kind` sans équivalent (les échantillons de déplacement `f012625` d'une course de
  route, par exemple) reste **non apparié** : le comparer à un `control` de la même étape
  comparerait deux instants différents ;
- `aligned 0` n'est jamais une réussite : le script affiche `NO PARITY` et sort en 1.

La sortie donne le nombre de paires et le nombre de checkpoints runtime **distincts**
utilisés, puis les non-appariés. Codes de sortie : `0` toutes les paires alignées sont
identiques et au moins une existe ; `1` une différence ou aucune paire ; `2` entrée
illisible ou ligne non JSON. Une différence ne change pas le code (il reste 1, il ne vaut
plus le nombre de différences). Un `daytime` ou un `hero` présent d'un seul côté échoue au
lieu d'être ignoré quand la comparaison le demande (`-HeroTolerance`).

Cas mesurés sur les preuves du 6 novembre 2026 (paires jetables sous
`build/v01-20261107/parity/throwaway/`) :

| Entrée | Avant | Après |
|---|---|---|
| clés disjointes | `aligned 0 ... OK` (code 0) | `NO PARITY` (code 1) |
| 2 originaux identiques, 1 runtime | `aligned 2 ... OK` (1 runtime réutilisé) | `aligned 1 ... with 1 distinct`, 1 non apparié |
| 2 originaux distincts, 2 runtime distincts | différence signalée (le premier runtime était réutilisé) | `aligned 2 ... with 2 distinct`, OK |
| `oracle-host` (2) contre le premier contrôle runtime frais (19) | `aligned 2`, code 2 | `aligned 1`, 1 non apparié, code 1, seule différence `4135` |
| `route1` (62 échantillons) contre le runtime frais | `aligned 62` contre un seul `control`, code 62 | `NO PARITY`, code 1 |

`route1-checkpoints.jsonl` est un échantillonnage de déplacement (un point toutes les 120
images), pas une suite de points d'histoire : il ne doit pas être compté comme 62 preuves.

## Comparer un état melonDS à un point : `scripts/oracle-compare.ps1`

Ce comparateur strict prend **un** état melonDS et **un** point runtime, soit un objet JSON
(`-RuntimeJson`, une seule ligne, pas un JSONL), soit le rapport texte de `--auto`
(`-Report` avec `-TrackIndex`). Il compare carte, piste courante, progression et drapeaux
des cinq pistes, drapeaux globaux, quêtes et `daytime`.

```powershell
target/release/dq9re.exe state build/orca/oracle/bat6.ml1 --json
# puis, avec la dernière ligne de --checkpoints seul dans un fichier :
powershell -ExecutionPolicy Bypass -File scripts/oracle-compare.ps1 `
    -State build/orca/oracle/bat6.ml1 `
    -RuntimeJson build/v01-20261107/parity/runtime-first-control-object.json `
    -Dq9re target/release/dq9re.exe
```

Au premier contrôle de l'Observatoire (`bat6.ml1`, vblank 26379, carte 4105, 1.1.1) :

```text
checkpoint: vblank 26379, map 4105, track 0 1.1.1
runtime: map 4105, tracks 5, global flags 17
DIFF global flags only in melonDS: 4212
```

Seul le butin aléatoire du combat 26 diffère : `4212` (slimedrop) dans `bat6.ml1`, `4135`
(herbe médicinale) dans `build/oracle-host/checkpoints.jsonl`. Ce n'est pas un échec
d'initialisation et il n'est ni forcé ni masqué. Depuis le 7 novembre 2026, un objet runtime
sans champ `daytime` ou `quests` produit une différence explicite (un champ absent ne vaut
pas une valeur vide), et un index de piste melonDS hors bornes est refusé. Le script sort en
0 ou 1, plus avec le nombre de différences.

**Convention de yaw** : `dq9re state --json` écrit l'angle brut du jeu (`Object3D+0x54`,
`/4096`), alors que `dq9-app --checkpoints` écrit le yaw applicatif. Les deux diffèrent de
`π` : au premier contrôle, `4.860` brut et `-1.718` applicatif, avec
`π - 4.860 = -1.718`. Les positions sont identiques ; seul le champ `yaw` n'est pas dans la
même convention. `compare-runs.ps1` ne compare que `x`, `y`, `z` (`-HeroTolerance`), donc il
n'est pas affecté ; une comparaison de yaw devra convertir l'un des deux.

## Déplacement à nombre d'images égal

Le champ se met à jour à 30 Hz ; `App::run_steps` fait désormais tourner le champ
(`Hero::step(..., 2)`, `Field::step`, démarche) un VBlank sur deux et tient la pose l'image
intermédiaire ([`movement.md`](movement.md#ce-qui-est-porté)). `--walk` passe par `run_steps`
et suit donc cette cadence. Les mesures de cette section datent de l'application à 60 Hz
(`ticks = 1`) : elles gardent leur valeur de comparaison historique, mais la conclusion a
changé et l'égalité stricte est maintenant vérifiée par un test natif sur ROM réelle
(`field_cadence_holds_the_pose_and_matches_the_original_every_vblank`, `crates/dq9-app/src/app.rs`).
Comparer deux moteurs à la même image demande de partir du **même état** : même position,
même vitesse (arrêtée) et **même orientation**.

Première tentative, depuis le premier contrôle sans aligner l'orientation
(`build/v01-20261107/parity/movement-up/`) : original `core23050.bin` (vblank 23033) avec
`up` tenu, RAM relevée à chaque image ; runtime avec `--start 0.66,-3.61` et 120 segments
`-z` d'une image. Le déplacement sur deux images est identique à 0,001 près sur 47 des 51
intervalles de l'image 20 à 120 (écart maximal 0,016 aux ruptures de pente ; pas stable
0,192 unité = 786 en virgule fixe), mais à l'image 120 l'original est à `z = -13.565` et le
runtime à `z = -13.495`. Cet écart venait du départ : `--walk` n'a pas d'option de yaw et
pose le héros face à `+z` (angle brut 0) alors que l'original regarde 4,860 ; la rotation
initiale et l'accélération ne se terminaient pas au même moment. Cette mesure ne prouvait
donc pas un défaut de cadence.

Deuxième expérience, orientation identique
(`build/v01-20261107/parity/movement-facing/`) :

```powershell
# original : tourner vers l'angle brut 0 (bas), s'arrêter, puis tenir up à l'image 70
python scripts/melondsds_host.py --rom <ROM> `
    --core tools/build/melondsds_libretro.dll `
    --state-in build/oracle-host/core23050.bin `
    --input build/v01-20261107/parity/movement-facing/prep.jsonl --frames 191 `
    --checkpoints build/v01-20261107/parity/movement-facing/original-prep.jsonl `
    --checkpoint-every 1 --dq9re target/release/dq9re.exe

$S = (1..120 | ForEach-Object { '-z:0.0166667' }) -join ','
target/release/dq9-app.exe --rom <ROM> --map 4105 --start 1.756,-3.041 `
    --walk $S --mute
```

L'original s'arrête à `(1.756, -0.250, -3.041)`, angle brut 0, vitesse nulle à l'image 70 ;
`--start 1.756,-3.041` pose exactement le même état (angle brut 0, vitesse nulle). Résultat
à la même image, sur 120 images de `up` :

- la trajectoire est la même : `x` identique, `y` à 0,003, `z` à 0,003 aux images 50, 100 et
  120 ; le héros s'arrête sur la même limite `z = -11.299` ;
- pendant la marche, l'écart suit la phase 30/60 Hz : l'original ne bouge qu'une image sur
  deux, l'application avance à chaque image. L'écart maximal est 0,191, soit exactement un
  pas de 30 Hz (786/4096 = 0,192), et il s'annule dès que les deux positions sont bloquées
  au même mur. Aucune dérive cumulée ;
- aux images paires (deux pas de 60 Hz = une mise à jour de 30 Hz), l'écart reste sous 0,10,
  la différence venant des arrondis de collision sur les pentes.

Conclusion de la mesure historique : l'application à 60 Hz suivait l'original au pas de
30 Hz, sans dérive cumulée, mais **l'égalité stricte à chaque VBlank n'était pas atteinte** :
l'application avançait à chaque image, l'original tient sa position une image sur deux.
L'écart mesuré était borné par un pas de champ (0,191 au maximum, 0,097 aux images paires,
0,003 à l'arrêt).

Le critère v0.1 « déplacement à nombre d'images contrôlé » est maintenant **fermé pour ce
segment** : le test natif sur ROM réelle rejoue exactement l'état de départ ci-dessus
(`(1,756 ; −3,041)`, angle brut 0, vitesse 0) et vérifie à chaque VBlank les angles bruts
exacts 0, 1608, 3216, …, 12868, les `z` fx de la capture originale (tolérance 2), la tenue
octet pour octet des images paires et la caméra qui continue de suivre la pose tenue. La
phase est celle de la revue : le premier VBlank ne lit que le pavé, le premier déplacement
est au troisième pas de champ (VBlank 5), le demi-tour se cale au VBlank 19. La cadence de
`Field::step` (portes) reste **[hypothèse]** — 4 appels à 30 Hz = 8 VBlanks — tant que la
mesure melonDS décrite dans [`movement.md`](movement.md) n'est pas faite. Aucun réglage CLI
n'a été ajouté.

Limites : `--walk` ne peut pas partir d'un yaw arbitraire, d'où la préparation originale
ci-dessus ; seuls les points du premier contrôle ont une référence originale dans les
preuves existantes ; l'orientation est comparée après conversion (angle brut original,
yaw applicatif).
