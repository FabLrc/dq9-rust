# Oracle melonDS : sauvegardes d'état (`.ml1`)

Module `dq9_formats::melonds`, commande `dq9re state <fichier>` (la ROM, si elle est trouvée,
sert à comparer le code résident), script `scripts/melonds-state.ps1` pour produire un état.

Objet : lire la mémoire d'un jeu qui tourne dans melonDS pour la comparer à ce que calcule
notre réimplémentation (état des triggers, équipe, position, progression). Le format est
celui de melonDS 1.x (« `MELN` ») ; sa description vient de
[ndskit](https://github.com/vs-sr-dev/ndskit) (`melonds.py`, MIT, vérifiée par son auteur sur
une version 2025) : **réimplémentée, pas copiée** ; le code de melonDS (GPL) n'a pas été lu.

## Format

Un en-tête de 16 octets (`MELN` puis version et taille), puis des sections consécutives :
nom sur 4 octets, longueur u32 **en-tête de 16 octets compris**, 8 octets d'en-tête de plus,
puis les données. On n'utilise que :

| Section | Décalage depuis le début de l'en-tête de section | Contenu |
|---|---:|---|
| `NDSG` | `0x14` | RAM principale (4 Mio utilisés ; la section en réserve 16 pour la DSi) |
| | `0x14 + 0x1000000` | WRAM partagée (32 Kio), puis WRAM de l'ARM7 (64 Kio) |
| `CP15` | `0x1c` | ITCM (32 Kio), puis DTCM (16 Kio) |

Un état troncaté ou sans `NDSG`/`CP15` assez longs est refusé (jamais de panique).

## Adresses utiles

| Adresse | Sens | Source |
|---|---|---|
| `0x027ffc3c` | compteur de VBlank du SDK : sert à aligner un état et une exécution sur la même image | ndskit |
| `0x020f33d8` | instance de `GameState` : la valeur est le mot du littéral lu par `GameState::GetInstance` (`0x0200f398`) | ARM9 européen, vérifié en RAM |
| `GameState+0x5cb0 / 4 / 8` | chapitre, étape, sous-étape (`Save_Chapter`, `Save_ScenarioStep`, `Save_ScenarioSubstep`) | `re/symbols.txt` |
| `0x02108844`, `0x02109418`, `0x020fb3f0` | `g_QuestManager` (bloc d'événements), `g_QuestLog`, `g_Zone3D` | `save.md` § « En RAM » |
| `GameState+8` | table de 0xe9 pointeurs d'objets de jeu (groupe, PNJ, monstres) : `Chara_Get` (`0x0200fd80`) | ARM9 européen ; `GameState.h` de la décompilation USA |
| `GameState+0x3ac` | indice de l'objet du protagoniste (`GameState::GetProtagonist`, `0x0200fddc`) | idem |
| objet `+0x44` | position dans le monde, 3 × fx32 (`Object3D::position_`) | décompilation USA (`Object3D.h`), vérifié en déplaçant le héros |

`State::read(addr, len)` adresse la RAM principale (miroir tous les 4 Mio), le DTCM à
`0x027e0000` et l'ITCM (miroir tous les 32 Kio) ; une lecture ne traverse pas deux mémoires.

## Utilisation

```powershell
cargo dq9re state partie.ml1                       # sections, VBlank, scénario, code vs ROM
cargo dq9re state partie.ml1 --save partie.sav     # + la RAM comparée à la sauvegarde chargée
cargo dq9re state partie.ml1 --peek 020f33d8:40    # octets en hexadécimal (adresse:longueur)
cargo dq9re state partie.ml1 --json                # checkpoint de l'histoire (JSON, voir plus bas)
cargo dq9re state --ram ram.bin --json             # même lecture sur une image RAM de 4 Mio
cargo dq9re states index build/oracle --out build/oracle/index.json
cargo dq9re states find --story 1.1.2 --map 4101   # catalogue : quels fichiers, sans les ouvrir
```

### État neutre : image RAM principale

`--ram FICHIER` lit une image **brute de la RAM principale** : exactement 4 Mio, à la base
`0x02000000`, sans en-tête (ce que `scripts/melondsds_host.py --dump-ram` écrit, et ce que
`retro_get_memory_data(RETRO_MEMORY_SYSTEM_RAM)` expose). Un chemin passé sans `--ram` est
aussi accepté : un fichier qui ne commence pas par `MELN` et fait exactement 4 Mio est lu
comme une image RAM (le contenu décide), ce qui laisse `scripts/oracle-compare.ps1`
fonctionner sur un dump sans le convertir.

Les deux conteneurs partagent la même vue mémoire (`dq9_formats::melonds::Memory`) : `read`,
`u32`, VBlank, scénario, bloc d'événements, carte, quêtes, objets et positions n'existent
qu'une fois, donc `--json` et `--peek` donnent **le même résultat** sur un `.ml1` et sur son
image RAM (vérifié sur le premier contrôle : `--peek 020f33d8:10` identique octet pour octet).
Une image RAM ne contient ni ITCM, ni DTCM, ni overlay : les lectures dans ces mémoires
échouent, et `dq9re state` l'annonce au lieu de comparer le code résident ; `--save` reste
disponible (toutes les copies de `SAVE_COPIES` sont en RAM principale).

### Catalogue (`dq9re states`)

`states index DOSSIER [--out index.json]` lit chaque fichier du dossier (pas des
sous-dossiers) : un `.ml1` est reconnu à `MELN`, une image RAM à ses 4 Mio exacts, tout le
reste est listé dans `invalid` avec son erreur au lieu de faire échouer le scan. Chaque entrée
donne `file`, `kind`, `vblank`, `map`, `area`, `track`, `story` (`CHAPITRE.ÉTAPE.SOUS-ÉTAPE`),
`flags` (nombre de drapeaux globaux) et `hero` (`x`, `y`, `z`, `yaw` en unités du monde). Un
état de reprise du cœur (`retro_serialize`, 19 Mio) commence aussi par `MELN` et se lit donc
comme un `.ml1`.

`states find [--story 1.1.2] [--map 4101] [--index index.json]` filtre ce catalogue
(`build/oracle/index.json` par défaut) et imprime une ligne par état : c'est la réponse à
« quel état est à 1.1.2 ? » sans charger chaque fichier. Exemple réel, sur les états
conservés du 2026-11-06 :

```
$ cargo dq9re states find --index build/oracle/index-preserved.json --map 4105
1.1.1     savestate map  4105 area  4100  (0.660, -0.250, -3.610) flags 18  vblank 31578    bat10.ml1
1.1.1     savestate map  4105 area  4100  (0.660, -0.250, -3.610) flags 18  vblank 23782    bat4.ml1
1.1.1     savestate map  4105 area  4100  (0.660, -0.250, -3.610) flags 18  vblank 26379    bat6.ml1
3 state(s) in …/build/orca/oracle
```

Avec la ROM, `dq9re state` compare au contenu de la ROM le module ARM9 statique, chaque bloc
autoload (ITCM, DTCM) et chaque overlay, et liste les overlays présents (≥ 90 % d'octets
identiques ; des overlays de même adresse ne se ressemblent pas à ce point). `ov_003` présent
= dans le jeu ; `ov_004` = écran titre.

`--save` compare chaque copie de `melonds::SAVE_COPIES` (où le chargement d'une sauvegarde
normale met ses octets, `save.md` § « En RAM ») à l'emplacement courant du `.sav` et signale
les écarts ; deux copies sont connues pour changer pendant le jeu (« changed by the game »).
Il compare aussi chaque membre du groupe à son objet en RAM (`State::compare_party`, champs
de `save.md` § « Membres du groupe »). Plusieurs écarts inattendus = le jeu n'a pas été
chargé depuis cette sauvegarde (sur l'écran titre : aucun objet membre n'a les bons ids).

Pour piloter le jeu (déplacements), associer d'abord des touches dans `melonDS.toml`
(`[Instance0.Keyboard]`, codes Qt : `Up = 16777235`, `Down = 16777237`, `Left = 16777234`,
`Right = 16777236`, `A = 88` (X), `B = 90` (Z)) ; des `WM_KEYDOWN`/`WM_KEYUP` postés à la
fenêtre suffisent alors, sans premier plan. Les modificateurs (Maj+F1) ne passent pas ainsi.

### Produire un état

```powershell
powershell -ExecutionPolicy Bypass -File scripts/melonds-state.ps1 -Save build/sav/demo.sav -Out build/melonds/demo.ml1
```

Le script (~40 s) prépare un melonDS portable dans `build/melonds/` (copie de
`tools/melonds/melonDS.exe`, fichier `portable`, ROM en lien physique, copie du `.sav`),
touche START sur l'écran titre (détecté à la couleur du bouton), puis « Continue your
adventure » **jusqu'à ce que les écrans changent**, attend `-Settle` secondes (8 par défaut)
dans le jeu, enregistre l'état 1 par *File → Save state → 1*, ferme melonDS (`-Keep` le laisse
ouvert) et lance `dq9re state --save`. Les entrées sont des messages postés à la fenêtre : elle
peut rester derrière d'autres fenêtres (Windows refuse le premier plan à un script quand
l'utilisateur travaille ailleurs, ce qui rendait Maj+F1 aléatoire). Il suppose la fenêtre par
défaut (échelle 1, écrans l'un sous l'autre). melonDS 1.1 n'associe aucune touche DS par
défaut : pour jouer au clavier, les régler une fois dans *Config → Input and hotkeys* (gardé
dans `build/melonds/melonDS.toml`). Le stub GDB de melonDS 1.1 (activé dans le `.toml`)
écoute bien sur 3333 mais a coupé la connexion dès le premier paquet d'un client RSP minimal
sous Windows ; non creusé, les états suffisent.

## Vérification

Sur des états réels produits depuis la sauvegarde de démonstration (`save.md`, melonDS 1.1,
démarrage direct, BIOS libres) :

- Les décalages de ndskit sont justes : module ARM9 statique (`0x02000000`, RAM principale de
  `NDSG`) et ITCM (`0x01ff8000`, `CP15`) identiques à 100 % à la ROM ; DTCM à 92,7 % (le
  reste est écrit à l'exécution) ; overlays chargés identiques à 100 % (`ov_003`, `ov_011`,
  `ov_017`, `ov_023`, `ov_032`, `ov_033` dans le jeu).
- `GameState` est bien en `0x020f33d8`, et toutes les copies de `SAVE_COPIES` se retrouvent
  octet pour octet (or, sac, bestiaire, alchimie, zone, cartes au trésor…), sauf les deux
  zones que le jeu modifie (`save.md` § « En RAM »). Les écarts sont les mêmes d'un état à
  l'autre.
- Compteur de VBlank cohérent avec 60 images/s (~2 100 après ~35 s).
- Positions : à l'arrivée dans l'église de Stornway (`C01M09`, id 109, identifié par
  `dq9re maps --state`), les 4 objets du groupe (indices 0 à 3,
  héros = 0) sont en (0,210 ; 0,120 ; −1,380). Après ~1 s vers le bas puis ~0,6 s vers la
  gauche (touches postées), le héros est en (−2,853 ; 0,106 ; 3,854) et les trois autres le
  suivent en file ; `dq9re state` les affiche. Les objets sont alloués dans le tas
  (`0x022bbbdc`, puis tous les `0x116c0`), d'où le passage obligé par la table. L'indice
  dans la table est l'id de l'ordre du groupe (`save.md` § « Membres du groupe ») : sur la
  démo, ordre `[0, 3, 2, 1]`, et l'id 3 (2ᵉ personnage) pointe vers le 2ᵉ objet alloué.
- Le vecteur de la sauvegarde copié en `g_Zone3D+0x2774` (−57 ; 0,1 ; −31,5) n'est pas la
  position du héros dans l'église : celle-ci est le point de départ de la carte (`.bmbl`,
  enregistrement `0x6e`), `save.md` § « Position à la reprise ». Comparaison automatisée :
  `dq9-core/tests/real_rom.rs` (`DQ9_SAVE`, `DQ9_STATE`).

**Correction** : la première version du script perdait parfois la confirmation de
« Continue » et enregistrait l'**écran titre** (`ov_004`) ; le scénario 19/2/1 y est déjà
posé par la lecture du journal d'aventure, ce qui l'avait fait passer pour un jeu chargé.
L'or et les autres blocs n'y sont pas : d'où la comparaison `--save`, qui distingue les deux
cas (15 copies différentes sur l'écran titre).

Les tests (`melonds::tests`) utilisent un état synthétique construit selon ce format.

## Oracle différentiel (v0.1)

But : comparer au jeu réel (melonDS) l'état de l'histoire que le runtime réimplémenté calcule,
aux **mêmes points** — progression des cinq pistes, leurs deux jeux de drapeaux, drapeaux
globaux, quêtes — plus la carte et la position du héros. Le critère de la v0.1
([`roadmap.md`](../roadmap.md), J3) est strict : mêmes drapeaux aux mêmes points ; il ne se
déduit ni de `--pilot` seul ni d'une sauvegarde fabriquée.

### Checkpoint (`dq9re state --json`)

Le bloc d'événements est lu en RAM à `g_QuestManager` (`0x02108844`) : même disposition que
la sauvegarde (`save.md`, § « Bloc d'événements »), donc pistes à `+0`, drapeaux globaux à
`+0x8c`, quêtes à `+0x2cc`, piste courante à `+0x332`. La carte vient de `Zone3D+0`/`+4`, le
numéro d'image du compteur VBlank (`0x027ffc3c`).

```json
{"vblank":26379,"map":4105,"area":4100,"track":0,"daytime":true,
 "tracks":[{"chapter":1,"step":1,"substep":1,"chapter_flags":[],"step_flags":[1]},
           {"chapter":1,"step":1,"substep":1,"chapter_flags":[],"step_flags":[]},
           {"chapter":1,"step":1,"substep":1,"chapter_flags":[],"step_flags":[]},
           {"chapter":1,"step":1,"substep":1,"chapter_flags":[],"step_flags":[]},
           {"chapter":1,"step":1,"substep":1,"chapter_flags":[],"step_flags":[]}],
 "flags":[337,1450,...],"quests":{}}
```

`flags` et les deux jeux d'une piste sont des **indices de bits** (bit de poids faible
d'abord, `Flags_Test`) ; `quests` ne liste que les quartets non nuls. Sans `--json`, la même
chose est imprimée en texte, avec la position des objets du groupe.

### Différentiel de deux états (`dq9re state diff`)

```powershell
cargo dq9re state diff bat6.ml1 bat4.ml1          # même point d'histoire -> code 0
cargo dq9re state diff pro1.ml1 bat6.ml1          # sortie 1 : cinq différences nommées
cargo dq9re state diff bat6.ml1 ram23050.bin --json
```

Les deux entrées peuvent être un `.ml1` ou une image RAM principale de 4 Mio
(`--dump-ram`) : le type est reconnu à l'en-tête `MELN` ou à la taille, jamais deviné. Le
lecteur est celui de `dq9re state` (`Memory`), donc les mêmes adresses et miroirs
s'appliquent.

Le premier bloc du rapport nomme les écarts **de jeu** : carte et area, piste courante,
histoire (`chapter.step.substep`), heure du jour, pose du héros (position et angle `fx32`),
progression et deux jeux de drapeaux de chacune des cinq pistes (bits ajoutés/retirés),
drapeaux globaux (ajoutés/retirés) et quêtes (quartets modifiés). `--json` les donne dans
`differences`, une entrée typée par champ (`{"field":"global_flags","change":{"added":[…],
"removed":[…]}}`).

Le second bloc résume **les autres octets** de la RAM principale, hors bloc d'événements
(déjà décrit ci-dessus) : nombre d'octets modifiés, puis des plages `début`, `longueur`,
`changed`. Les octets égaux qui séparent deux changements de moins de 16 octets fusionnent
dans une même plage, pour lire une structure d'un bloc ; le nombre `changed` reste exact.
En texte, seules les 200 premières plages sont imprimées (`--json` les garde toutes). Le
compteur de VBlank, les horloges et le tas y apparaissent : ce sont des différences
transitoires, pas des écarts de jeu.

Codes de sortie : **0** si le premier bloc est vide (état de jeu identique, même si des
octets transitoires diffèrent), **1** si au moins un champ de jeu diffère, **2** si une
entrée est illisible ou n'est ni un état melonDS ni une image de 4 Mio. C'est ce que
vérifie le smoke du 2026-11-06 : `bat6.ml1` contre `bat4.ml1` (même premier contrôle,
images différentes) donne 0 et 136 plages transitoires ; `pro1.ml1` contre `bat6.ml1`
donne 1 avec carte, area, pose, `step_flags` et drapeaux globaux nommés.

Limite : le diff voit les octets qui changent et les champs qu'ils composent ; il ne dit
pas **qui** les a écrits. Un drapeau posé directement hors de `Flags_Set` est donc vu, son
auteur ne l'est pas (voir `state watch` ci-dessous pour la provenance image par image).

### Traçage image par image (`--watch` + `dq9re state watch`)

Le harnais libretro (`scripts/melondsds_host.py`, § « Harnais libretro ») peut surveiller des
octets de la RAM principale et n'écrire que leurs changements :

```powershell
python scripts/melondsds_host.py --rom <ROM> --frames 12000 `
    --input build/oracle/inputs/prologue.jsonl `
    --watch 0x02108844:0x334 --watch-out build/oracle/watch.jsonl
cargo dq9re state watch --file build/oracle/watch.jsonl
cargo dq9re state watch --file build/oracle/watch.jsonl --json
```

`--watch ADDR:LEN` est répétable, en hexadécimal, et les zones qui se chevauchent sont
fusionnées. Chaque octet modifié produit une ligne
`{"frame":N,"addr":"0x...","old":0,"new":1}` ; `frame` compte les images exécutées depuis le
début, la première photo étant prise avant l'image 1. Seules les zones surveillées sont
copiées à chaque image (quelques Kio), pas les 4 Mio.

`dq9re state watch` nomme chaque changement quand l'adresse tombe dans le bloc
d'événements (`g_QuestManager`, `0x02108844`) : bit de drapeau global, bit d'un des deux
jeux d'une piste, chapitre/étape/sous-étape, quartet de quête, piste courante. Les autres
adresses restent des octets de RAM nommés par leur adresse. Le texte donne une ligne par
changement et un total ; `--json` renvoie un tableau d'objets typés (`kind` :
`global_flag`, `track_flags`, `track_progress`, `quest`, `current_track`, `ram`). Une ligne
invalide fait échouer la commande (code 2) sans rien inventer.

Exemple de sortie sur le bloc d'événements :

```text
frame 1450: global flag 1450: 0 -> 1
frame 7401: global flag 1451: 0 -> 1
frame 9000: track 0 step_flags flag 1: 0 -> 1
frame 9001: current track: 0 -> 1
```

Limites : le log voit le **changement**, pas l'instruction qui l'a écrit ; pour le `LR` de
l'écriture, `DQIX_WATCH` de DQIX Enhanced est la piste documentée dans
[`outils.md`](outils.md). Un octet qui change plusieurs fois entre deux images ne garde que
la dernière valeur ; `--checkpoint-every` et `--dump-ram` donnent l'état complet aux images
choisies.

### Côté runtime

`dq9-app --new-game --headless --auto` s'arrête à la première prise de contrôle et imprime le
journal puis, pour chaque piste, `track i: c.s.b flags [{...}, {...}]`, `global flags: {...}`
et la position du héros. `--pilot` continue désormais par la marche et les interactions
réelles, sans téléportation : le parcours vérifié atteint 1.1.2 dans `X01M01`, pas encore
le chapitre 2. `--checkpoints FICHIER` écrit le JSONL (mêmes noms de champs plus
`tick`/`seconds`/`kind`/`event`/`hero`) ; `-RuntimeJson` compare un objet de ce fichier.
`--trace-inputs FICHIER` et `--replay FICHIER` reproduisent le parcours du pilote à caméra
d'angle 0 ; cette preuve ne constitue ni un rejeu de caméra manuelle ni la parité melonDS.

### Comparateur

```powershell
# rapport texte : le rapport ne nomme pas la piste courante, il faut la donner
powershell -ExecutionPolicy Bypass -File scripts/oracle-compare.ps1 `
    -State build/melonds-oracle/observatory.ml1 -Report build/oracle/runtime-auto.txt -TrackIndex 0
# contrat JSONL du runtime : un seul objet (une ligne de --checkpoints)
powershell -ExecutionPolicy Bypass -File scripts/oracle-compare.ps1 `
    -State build/melonds-oracle/observatory.ml1 -RuntimeJson build/oracle/checkpoint.json
```

Il exécute `dq9re state --json` sur l'état, puis compare **strictement** la carte, l'indice de
la piste courante, le nombre de pistes, la progression et les deux jeux de drapeaux de chaque
piste, les drapeaux globaux, et — seulement quand `-RuntimeJson` les fournit — `daytime` et
les quêtes (le rapport texte ne les contient pas). Une piste courante absente du rapport
texte **ou de l'objet JSON** est un écart (code non nul), jamais un `OK` : c'est `-TrackIndex`
ou `-RuntimeJson` qui la donne. Des quêtes non nulles côté melonDS sans quêtes côté runtime
sont aussi un écart. Code de sortie non nul et une ligne `DIFF` par écart ; les ensembles
vides sont valides (pas de test de véracité sur un ensemble).

### Piloter melonDS (`scripts/melonds-play.ps1`)

Le script copie `melonDS.exe` dans `build/melonds-oracle/` (avec `portable`, un lien physique
vers la ROM, `dq9.sav`), écrit un `melonDS.toml` valide (sans BOM) avec les touches
(`Up`/`Down`/`Left`/`Right`, `A = X`, `B = Z`, `Start = Retour`), puis poste les messages à
la fenêtre (pas de premier plan, fenêtre non redimensionnée). Actions : `waitstart` (bouton
START orange), `tap`, `key`, `hold:up+left:2`, `spam`/`play` (A toutes les N secondes, états
tous les M), `save`, `loadstate`, `shot`, `pause`/`frame:N` (hotkeys melonDS, voir les
limites). Exemple de référence nouvelle partie :

```powershell
# écran titre -> menu -> création du personnage (A x10, OK tactile, « Yes ») -> prologue
powershell -ExecutionPolicy Bypass -File scripts/melonds-play.ps1 -Fresh -Actions `
  "waitstart,tap:127:377,wait:8,key:a,wait:3,key:a,wait:3,key:a,wait:3,key:a,wait:3,key:a,wait:3,key:a,wait:3,key:a,wait:3,key:a,wait:3,key:a,wait:3,key:a,wait:3,tap:221:144,wait:3,key:a,wait:8,play:260:2:20:pro,shot:end"
# reprise d'un état (File > Load state > 1) puis combat/scènes
powershell -ExecutionPolicy Bypass -File scripts/melonds-play.ps1 `
  -LoadState build/melonds-oracle/pro11.ml1 -Actions "wait:3,loadstate,wait:2,play:240:1.5:20:bat"
```

La création du personnage est réelle (aucune sauvegarde fabriquée) : les défauts sont
acceptés, le nom est un caractère tapé au clavier tactile, puis « Begin your adventure » ;
le combat 26 est joué (A) et gagné normalement. Les états sont des points d'histoire stables,
pas des images exactes.

### Harnais du cœur melonDS DS (libretro)

Le melonDS autonome exige une fenêtre focalisable et son stub GDB se déconnecte ; le cœur
officiel [melonDS DS](https://docs.libretro.com/library/melonds_ds/) exécute la ROM par
callbacks, sans fenêtre ni messages postés. `scripts/melondsds_host.py` l'appelle par son ABI
C (ctypes) : `retro_set_environment`/`video_refresh`/`audio_sample_batch`/`input_state`,
`retro_run` **une image à la fois**, `retro_serialize`/`retro_unserialize`,
`retro_get_memory_data(RETRO_MEMORY_SYSTEM_RAM)`. Le cœur (GPL, extérieur au dépôt) et la ROM
restent hors worktree : `--core tools/build/melondsds_libretro.dll` (archive de référence,
SHA-256 `b2873cc3…`), `--rom` ou `DQ9_ROM`.

```powershell
python scripts/melondsds_host.py --rom <ROM> --core <melondsds_libretro.dll> `
    --input run.jsonl --frames 23050 --checkpoints build/oracle/checkpoints.jsonl `
    --dump-ram build/oracle/ram.bin --state-out build/oracle/core.bin --shot build/oracle/control.png
```

Options : `--rom`, `--core`, `--sysdir`, `--savedir`, `--input`, `--frames`, `--checkpoints`,
`--checkpoint-ram-dir` (garde les images RAM de chaque point), `--checkpoint-every N`,
`--dq9re` (binaire, sinon `target/release` puis `target/debug`), `--dump-ram` (4 Mio),
`--state-out`/`--state-in` (état du cœur, donc reprise possible d'une course réelle),
`--shot`, `--shot-every N`/`--shot-dir`, `--core-log` (journal du cœur, sinon jeté). Le firmware
est celui du cœur (`melonds_sysfile_mode = builtin`), aucun BIOS n'est requis.

Entrées (`--input`, JSONL, un objet par ligne, **croissant par index d'image**, tenu jusqu'à
la ligne suivante qui le change — donc un fichier clairsemé suffit, et une ligne par image
donne un contrôle image par image) :

```json
{"frame": 1300, "pad": ["start"]}
{"frame": 1310, "pad": []}
{"frame": 1700, "touch": [128, 305]}
{"frame": 1710, "touch": null, "checkpoint": "creation"}
{"frame": 2200, "shot": "build/oracle/creation.png"}
```

- `frame` : nombre d'appels à `retro_run` déjà faits (omis = ligne précédente + 1) ;
- `pad` : `a b x y l r start select up down left right`, remplace l'ensemble tenu (`[]`
  relâche) ;
- `touch` : `[x, y]` en **pixels de l'image** (256×384, écran du haut puis du bas ; le harnais
  convertit vers la plage pointeur de libretro `-0x8000..0x7fff`, sans quoi le cœur ne reçoit
  aucun appui), ou `null` pour lever le stylet ;
- `checkpoint` : capture un point avant que l'image ne tourne (`true` ou un nom de `kind`) ;
- `shot` : PNG avant que l'image ne tourne.

`--checkpoints` écrit le JSONL du contrat de `dq9-app --checkpoints` : `tick` (index d'image),
`seconds` (`tick/60`), `kind`, puis les champs de `dq9re state --json` (`vblank`, `map`,
`area`, `track`, `daytime`, `tracks`, `flags`, `quests`, `hero`), `event` restant `null`
(l'original n'expose pas l'id de scène). Chaque point passe par `dq9re state --ram` sur une
image RAM temporaire : **une seule** implémentation des offsets, et `--checkpoint-ram-dir`
les garde pour le catalogue ou pour `oracle-compare.ps1`.

Limites : l'image vidéo capturée est celle de la **dernière** image exécutée ; seuls joypad
et stylet sont fournis (pas de micro, pas de Wi-Fi, pas de RTC réglée). L'A en boucle ferme
bien les messages, mais il peut **rouvrir** une conversation de PNJ répétable une fois la
scène finie : la progression de l'histoire reste alors identique, ce n'est pas un blocage de
la fenêtre de message (preuve en 2026-11-07 ci-dessous).

### Reprise d'état, fenêtre de message et coordonnées tactiles (2026-11-07)

`--state-in` reprend exactement un fichier écrit par `--state-out` (même cœur) : les
milestones d'une course découpée en segments se reproduisent dans une seule course continue
(§ « Course originale continue » ci-dessous). Le harnais expose aussi la RAM principale, donc
l'état de la fenêtre de message se lit **sans capture** : `W = 020421b0()`, c'est-à-dire
`u32[u32[0x020421bc] + 0x1c]` ; le 2026-11-07, `W = 0x021152f4` dans les deux états
`build/orca/route/idle-state.bin` et `build/v01-20261107/oracle/end1600-state.bin`.

| Champ | Sens |
|---|---|
| `W+0x998` | fenêtre active (1) ou fermée (0) |
| `W+0x9a0` | état d'attente de `MsgWin_Update` (0 attend la touche, 3 `<ADD>`, 5 frappe, 6 choix) |
| `W+0x19bd` | octet rendu par la native 405 (`get_message_state`) : 1 tant que le message n'est pas refermé |
| `W+0x912` | 3 = flèche d'attente affichée ; 5 alterne pendant la frappe |
| `W+0x954` | réponse du dernier `YESNO` : −1 pas de réponse, 0 oui, 1 non |

Mesure : depuis `idle-state.bin`, un A à l'image 61 fait passer `W+0x998` et `W+0x19bd` de 1 à
0 à l'image 67 ; la scène affiche alors son message suivant à l'image 110 et se termine à
l'image 238. L'appui A est donc reçu et le texte se ferme.

**Fausse impasse après 1160.** Le message « Stop! Benevolence is not a toy! » n'appartient pas
à l'événement 1160 : c'est l'entrée 169 de `data/event/ev01600/ev01600_en.bin` (clés 168, 169,
170), la scène de l'Observatoire où un compagnon demande à recevoir la bienveillance
(`data/scenario/X01A0/130_en.bin`, PNJ 130 de `X01M07` en (−9,27 ; 0,25 ; 1,16)). Une fois 1160
terminé, le héros est en contrôle libre ; les A en boucle rouvrent la conversation répétable
du PNJ 130, dont la scène 1600 se rejoue (le choix « Yes » puis 168 → 169 → 170) toutes les
60 à 80 images, et l'histoire reste en 1.1.1. Aucun état n'était bloqué : il fallait **arrêter
les A et marcher**. Preuve : un A à l'image 61 ferme la fenêtre et laisse le contrôle libre à
l'image 238 ; `--watch 0x021152f4:0x1a00` montre 998 qui repasse de 0 à 1 à chaque nouvelle
conversation.

**Coordonnées tactiles.** `--input` attend `[x, y]` sur l'image **empilée 256 × 384** (haut
puis bas), comme documenté plus haut ; l'écran tactile est la partie basse, donc `y` doit être
dans `192..383`. Les essais antérieurs `[128, 160]` visaient l'écran du haut et ne pouvaient
pas atteindre le stylet ; ils ne prouvent rien sur la vitesse du texte ni sur une attente
tactile.

### Course originale continue : 1160 → 1170 → 1.1.2 (2026-11-07)

Entrées et preuves (ignorées par git) : `build/v01-20261107/oracle/`. La course part de
`build/orca/route/idle-state.bin`, ferme la scène 1600, marche jusqu'à `X01M01`, parle au
PNJ 127 (trigger `room 4101, event 0 : 0x6[?](127) 0x4[?](3) 0x5[?](4) → 0x77(1170)`), joue
1170, puis s'arrête à la première image de 1.1.2.

```powershell
python scripts/melondsds_host.py --rom <ROM> --core tools/build/melondsds_libretro.dll `
    --state-in build/orca/route/idle-state.bin `
    --input build/v01-20261107/oracle/continuous.jsonl --frames 3966 `
    --checkpoints build/v01-20261107/oracle/continuous-cp.jsonl `
    --state-out build/v01-20261107/oracle/continuous-state.bin `
    --dump-ram build/v01-20261107/oracle/continuous-ram.bin `
    --shot build/v01-20261107/oracle/continuous-112.png
cargo dq9re map X01M01 --state build/v01-20261107/oracle/continuous-state.bin
cargo dq9re state build/v01-20261107/oracle/continuous-state.bin --json
```

Repères de la course (image, pose du héros, carte) :

| Image | Pose | Point |
|---:|---|---|
| 800 | (−8,275 ; 0,169 ; 1,257) | contrôle libre après 1600, `X01M07` 1.1.1 |
| 1100 | (−8,275 ; 0,169 ; 5,957) | descente plein sud |
| 1200 | (0,985 ; 0,169 ; 5,957) | passage à l'est |
| 1440 | (1,177 ; 0,169 ; 6,5) | mur sud |
| 1940 | (−7,32 ; 0,169 ; 14,98) | couloir ouest puis sud |
| 2200 | (0 ; 4,25 ; 31,72) | entrée `X01M01` par la porte de `X01M07` |
| 2590 | (−3,509 ; 7,843 ; 28,514) | rampe sud-ouest |
| 3010 | (2,704 ; 11,249 ; 22,103) | plateforme haute |
| 3230 | (10,125 ; 11,249 ; 15,937) | côté est de la plateforme |
| 3400 | (1,68 ; 11,249 ; 21,919) | retour au centre |
| 3480 | (1,303 ; 11,249 ; 18,651) | A devant le PNJ 127 |
| 3571 | (1,303 ; 11,249 ; 18,651) | message « Apus Major has sent you… » |
| 3965 | (2,77 ; 11,25 ; 18,19) | **première image de 1.1.2** |

À l'image 3965, `dq9re state --json` donne carte `4101`, area `4100`, piste 0,
`chapter/step/substep = 1.1.2`, `step_flags {1,2,3,4}`, drapeaux globaux inchangés
(`1450–1454,1461,2721,2751,2774,3191,3775,3955,4046,4135,4283,4410,4414`), héros
(2,77 ; 11,25 ; 18,19), cap 1,888.

Le trigger de fin de 1170 exécute `0x68(4)` (drapeau de piste), `0x95(0, 0x100000)` puis
`0x84(0,1,1,2)` (avance 1.1.2). Dans l'original, `dq9re map X01M01 --state` nomme l'effet du
`0x95` : sur `continuous-state.bin`, « block 0 (X01M0100.bmdj): instance 16 hidden » ; sur
`talk127b-state.bin` (1.1.1, avant 1170), « the loaded blocks match the assembly ». L'instance
16 est donc bien **masquée à 1.1.2** par le jeu : un runtime qui ignore `0x95` garde le mur
mince S1 et ne peut pas prétendre au même état.

**3965 est une frontière de progression, pas une prise de contrôle.** Le héros reste bloqué
environ 60 à 80 images après l'avance de piste (aucune direction ne le déplace à l'image
3965) ; c'est pour cela que le point de contrôle de progression ne doit pas être comparé à
une prise de contrôle du runtime. `control112-state.bin` est l'état **contrôlable** après
90 images sans entrée : même pose, fenêtre de message fermée, et `Left` 40 images déplace
le héros en (−0,875 ; 11,65 ; 17,853).

#### Mur S1 et tour `X01M01` → `X01M03` → `X01M09`

La pièce 12 masquée est `X01A01S1.col2` (2 triangles, `184` octets) : un mur mince à
x `0,0186..0,1055`, y `9..14`, z `15,42..18,99`, dans les coordonnées de la carte.

- **1.1.1 (mur présent)** : depuis `seg11-state.bin` (1,68 ; 11,25 ; 21,19), `Up` 44 images
  puis `Left` s'arrête à (0,947 ; 11,25 ; 18,557) ; 60 images de `Left` supplémentaires ne
  déplacent plus le héros.
- **1.1.2 (mur masqué)** : depuis `control112-state.bin`, `Left` traverse x = 0,9 et le
  héros monte : (−0,144 ; 11,459 ; 17,944) à l'image 4096, puis (−7,432 ; 13,727 ; 16,402)
  quand la carte passe à `4103` (`X01M03`). Il arrive en (−9,75 ; 4,25 ; 17,62), puis
  `Up` atteint `4109` (`X01M09`) à l'image 4376 en (−9,236 ; 4,25 ; 11,727). L'identifiant de
  carte change quelques images avant le déplacement de la pose : le point stable d'arrivée
  est (−9,75 ; 4,25 ; 17,62), à comparer après la transition.

Course continue étendue (`continuous2.jsonl`, même départ `idle-state.bin`) :

```powershell
python scripts/melondsds_host.py --rom <ROM> --core tools/build/melondsds_libretro.dll `
    --state-in build/orca/route/idle-state.bin `
    --input build/v01-20261107/oracle/continuous2.jsonl --frames 4416 `
    --checkpoints build/v01-20261107/oracle/continuous2-cp.jsonl `
    --state-out build/v01-20261107/oracle/continuous2-state.bin `
    --dump-ram build/v01-20261107/oracle/continuous2-ram.bin `
    --shot build/v01-20261107/oracle/x01m09-4415.png
```

| Image | Carte | Pose | Point |
|---:|---:|---|---|
| 3965 | 4101 | (2,77 ; 11,25 ; 18,19) | 1.1.2, contrôle encore verrouillé |
| 4055 | 4101 | (2,77 ; 11,25 ; 18,19) | contrôle réel, message fermé |
| 4096 | 4101 | (−0,144 ; 11,459 ; 17,944) | traverse le mur S1 masqué |
| 4196 | 4103 | (−7,432 ; 13,727 ; 16,402) | carte `X01M03` (pose pas encore déplacée) |
| 4236 | 4103 | (−9,75 ; 4,25 ; 17,62) | arrivée stable dans `X01M03` |
| 4376 | 4109 | (−9,236 ; 4,25 ; 11,727) | `X01M09` |
| 4415 | 4109 | (−7,36 ; 0,199 ; 10,22) | fin de la course rejouée |

Empreintes SHA-256 : `continuous.jsonl` `3d0fd18d…`, checkpoints `469404ea…`, état
`496fec9e…`, image RAM `cc30d043…`, capture `fd0c728e…`, `idle-state.bin` `cad28ef0…`,
état 1.1.1 `talk127b-state.bin` `efd0ff3e…`, `continuous2.jsonl` `76e11b30…`, checkpoints
`d0ef2a09…`, état `11411a78…`, image RAM `be2765ce…`, `control112-state.bin` `3714213d…`,
capture `X01M09` `b85b8730…`.

### Mesure du 2026-11-06 (ROM EU, melonDS 1.1, BIOS libres)

Référence : nouvelle partie réelle, combat 26 gagné, états pris toutes les 20 s. Le premier
contrôle de l'Observatoire donne (vblank 26379, `bat6.ml1`) :

| | melonDS | runtime `--new-game --headless --auto` (210,42 s) |
|---|---|---|
| carte | `4105` (`X01M05`) | `4105` |
| héros | (0,660 ; −0,250 ; −3,610) | (0,6599 ; −0,25 ; −3,6099) |
| histoire | 1.1.1, piste 0 | 1.1.1, piste 0 |
| `step_flags` piste 0 | `{1}` | `{1}` |
| drapeaux globaux | 18 | 1 (`337`) |

Identiques : carte, position (à moins de 10⁻³), progression et drapeaux de piste. **Écart
mesuré** : 17 drapeaux globaux présents dans melonDS et absents du runtime —
`1450,1451,1452,1453,1454,1461,2721,2751,2774,3191,3775,3955,4046,4212,4283,4410,4414` ;
`337` est commun. `GameState::new_game` laisse le bloc d'événements vide
(`state.rs`, « the game's initial tracks are not read ») : l'écart couvre donc l'initialisation
de la nouvelle partie, les scènes 21500–21596 et la victoire du combat 26 (substituée côté
runtime). Signalé aux workers flags et parcours avec l'état et le comparateur ; accepté comme
ground truth, non corrigé ici.

**Depuis (6 novembre 2026), les causes sont branchées** : équipement initial (item
`0xc76 + compendium`), chargement de `S14M05` avant le prologue, `0x72(22159)`/`0x6b` du
trigger de fin de `21596`, touche tenue. Le runtime pose 17 drapeaux ; seul le butin aléatoire
du combat 26 (`4135` herbe médicinale, ou `4212` slimedrop selon la course) reste absent
([`trigger.md`](trigger.md), [`item.md`](item.md)).

Pendant l'ouverture (`M01M12`, 1.1.1) les mêmes 7 drapeaux de base sont déjà posés au premier
état (vblank 5217) et `1450`, puis `1451`, apparaissent pendant la scène : ce sont des
candidats « initialisation/scripts », pas des drapeaux de piste.

Le runtime a livré ses checkpoints (`--checkpoints`, JSONL, mêmes noms de champs).
Repères pendant l'ouverture : **les états originaux ci-dessous ne sont pas alignés sur les
fins d'événements du runtime**. La même progression 1.1.1 ne suffit pas à établir qu'ils
décrivent le même instant de la scène ; ce tableau ne valide pas la parité des checkpoints.

| point | runtime | melonDS |
|---|---|---|
| fin de `21500` (tick 4837, `M01M12`, 1.1.1) | `[]` | `pro1` : `2721,2774,3191,3775,3955,4046,4414` |
| fin de `21507` (tick 7401, même carte) | `[]` | `pro7` : les 7 + `1450,1451` |
| premier contrôle (tick 12625, `X01M05`) | `[337]` | `bat6` : 18 drapeaux |

Une partie de l'écart existe donc dès l'ouverture, avant le combat 26 ; l'origine des autres
drapeaux reste à établir. Le runtime efface ensuite `337` à `X01M07` (tick 18482) ; aucun état
melonDS à cet endroit pour trancher.

**Franchissement du premier contrôle (relevé réel, reproductible).** Depuis
(0,660 ; −0,250 ; −3,610) : `Up` 8 s (escaliers), `Left` 4 s → (−16,45 ; 2,25 ; −17,47),
`Down` 4 s (balcon) → (−16,45 ; 2,25 ; 5,75), `Right` 1,1 s + `Down` 0,3 s →
(−10,77 ; 2,25 ; 8,16), puis A : l'event 1140 se joue avec le PNJ 111, à
(−11,27 ; 2,25 ; 9,04). La conversation se ferme après **8** pressions A (la 9ᵉ la rouvre) ;
le balcon gauche s'arrête à z = 9,5, le PNJ 105 est en bas à (−18,5 ; 0,45 ; 20,31). Ces
coordonnées sont partagées avec le worker du parcours ; la suite (tour `X01M03`, warp
y = 15,5) reste à franchir en marche réelle.

#### Premier contrôle par le cœur libretro (2026-11-06)

Course réelle rejouée par `scripts/melondsds_host.py` (ROM EU, cœur 1.4.0, firmware intégré) :
écran titre (`start` à l'image 1300), menu, création du personnage, « Yes », puis A toutes
les 24 images jusqu'à l'image 23050. Le fichier d'entrées fait 1698 lignes ; les appuis
clavier sont `start` (10 images) puis `a` (6 images toutes les 24), et un seul appui tactile
(`[128, 305]`, la ligne « Female », images 1700–1710). Les champs de création sont donc
laissés à leurs valeurs courantes, à l'exception du sexe, et le nom devient `11111` (un A par
caractère). Commande :

```powershell
python scripts/melondsds_host.py --rom <ROM> --core <melondsds_libretro.dll> `
    --input build/tmp/in11.jsonl --frames 23050 `
    --checkpoints build/oracle/checkpoints.jsonl --dump-ram build/oracle/ram23050.bin `
    --state-out build/oracle/core23050.bin --shot build/oracle/control.png
cargo dq9re state --ram build/oracle/ram23050.bin --json
```

| | `bat6.ml1` (melonDS 1.1) | image RAM du cœur (vblank 23033) |
|---|---|---|
| carte, area | `4105`, `4100` | `4105`, `4100` |
| histoire, piste | 1.1.1, piste 0 | 1.1.1, piste 0 |
| `step_flags` piste 0 | `{1}` | `{1}` |
| héros (fx32 → monde) | (0,660 ; −0,250 ; −3,610), cap 4,86 | (0,660 ; −0,250 ; −3,610), cap 4,86 |
| drapeaux globaux | 18 | 18 |

Identiques : carte, area, progression, drapeaux de piste, position et cap du héros (mêmes
fx32), et le nombre de drapeaux globaux. `scripts/oracle-compare.ps1` sur l'image RAM et ce
point de contrôle répond `OK: story point and flags identical` ; contre `bat6.ml1` il ne
signale que **deux** écarts, un bit de chaque côté :

```
DIFF global flags only in melonDS: 4212
DIFF global flags only in the runtime: 4135
```

`4135` n'est pas une constante : deux autres courses réelles, identiques sauf le sexe du
héros (aucun appui tactile, ou appui sur « Male »), ont **17** drapeaux, sans `4135` ni
`4212` — l'apparence du membre 0 y est octet pour octet celle de `bat6`
(`cf32573f40232823…`). `4212` est en revanche présent dans les trois états `bat*.ml1` (18
drapeaux, vblank 23782/26379/31578) et déjà dans `pro11.ml1` (vblank 18384, carte 20101), donc
posé tôt dans le prologue de la course autonome : la branche qui le pose n'est pas encore
établie et **aucun drapeau n'a été forcé** pour la faire coïncider. `bat6` a par ailleurs été
pris 3346 images (VBlank) plus tard que l'image ci-dessus, dans la même scène : les deux
points ne sont pas le même instant.

### Limites

- La création du personnage est pilotée au temps réel (A, OK tactile) : la séquence est
  reproductible mais pas à l'image près ; les points de contrôle sont lus en RAM, jamais
  déduits d'une capture d'écran. Avec le harnais libretro elle est à l'image près, mais la
  cadence des A (24 images) reste un choix, pas le rythme d'un joueur.
- Le A en boucle avance les dialogues mais **rouvre** la conversation à la fin (le héros fait
  toujours face au PNJ) : il ne suffit pas à progresser. Au-delà du premier contrôle,
  atteindre Angel Falls exige le **vrai chemin physique** (escaliers, portes, PNJ à qui
  parler). Les entrées du parcours réel du worker field seront rejouées dans melonDS ; aucun
  téléport ne servira de référence.
- Le combat 26 est un vrai combat (pas de contournement), mais le runtime le substitue : les
  drapeaux posés par le combat/le moteur peuvent manquer (c'est l'écart ci-dessus).
- Aucun quartet de quête à ces points ; les checkpoints « quêtes du village » et « fin de
  prologue » attendent le chemin physique.
- La sauvegarde d'état passe par le menu (environ 2 s, temps réel) ; ce sont des points
  d'histoire stables, pas des numéros d'image alignés.
- Le pas à pas (`pause` + `frame:N`, hotkeys `HK_Pause`/`HK_FrameStep`) fonctionne mais
  melonDS 1.1 n'avance qu'environ **0,5 image par pression** sous messages postés (mesuré :
  +3 VBlank pour 10 pressions, +25 pour 50) : le nombre d'images réel se lit au compteur
  VBlank, et ce n'est pas encore assez exact pour le critère J2 « même nombre d'images ».

## Suite

- Rejouer les entrées du parcours réel (premier contrôle → Angel Falls → quêtes → fin du
  prologue) dans le **cœur libretro** : le harnais fournit les mêmes touches image par image,
  donc le critère J2 « même nombre d'images » devient atteignable en traduisant une trace
  `dq9-app --trace-inputs` en JSONL d'entrées. melonDS 1.1 autonome, lui, n'avance que ~0,5
  image par pression sous messages postés (`pause`/`frame:N`).
- `4212` et `4135` : **provenance établie** (6 novembre 2026). Ce sont des drapeaux d'objet
  obtenu (`func_0207cc18`, `0xc76 + indice de compendium`) : l'herbe médicinale 22000
  (indice 945 → 4135) dans la course féminine, le slimedrop 22087 (indice 1022 → 4212) dans
  `bat6`. Le butin du combat 26 est **aléatoire** : trois courses aux entrées identiques sauf
  le sexe donnent l'herbe ou aucun des deux. Le runtime substitue le combat et ne tire pas de
  butin ([`item.md`](item.md), [`trigger.md`](trigger.md)).
- L'état contient aussi l'ARM7 et le son (`SPU.`) : hors périmètre tant que l'audio n'est pas
  porté.
