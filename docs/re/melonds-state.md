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

### Côté runtime

`dq9-app --new-game --headless --auto` s'arrête à la première prise de contrôle et imprime le
journal puis, pour chaque piste, `track i: c.s.b flags [{...}, {...}]`, `global flags: {...}`
et la position du héros. `--pilot` continue (téléportations : ordre de l'histoire seulement).
Le worker du parcours ajoute `--checkpoints FICHIER` (JSONL, mêmes noms de champs plus
`tick`/`seconds`/`kind`/`event`/`hero`) ; le comparateur lit le rapport texte en attendant.

### Comparateur

```powershell
powershell -ExecutionPolicy Bypass -File scripts/oracle-compare.ps1 `
    -State build/melonds-oracle/observatory.ml1 -Report build/oracle/runtime-auto.txt
```

Il exécute `dq9re state --json` sur l'état, lit la dernière ligne de chaque piste et les
drapeaux globaux du rapport, puis compare carte, progression et les deux jeux de drapeaux de
chaque piste et les drapeaux globaux ; code de sortie non nul et une ligne `DIFF` par écart.
Les quêtes ne sont pas encore dans le rapport du runtime : elles ne sont pas comparées.

### Piloter melonDS (`scripts/melonds-play.ps1`)

Le script copie `melonDS.exe` dans `build/melonds-oracle/` (avec `portable`, un lien physique
vers la ROM, `dq9.sav`), écrit un `melonDS.toml` valide (sans BOM) avec les touches
(`Up`/`Down`/`Left`/`Right`, `A = X`, `B = Z`, `Start = Retour`), puis poste les messages à
la fenêtre (pas de premier plan, fenêtre non redimensionnée). Actions : `waitstart` (bouton
START orange), `tap`, `key`, `hold:up+left:2`, `spam`/`play` (A toutes les N secondes, états
tous les M), `save`, `loadstate`, `shot`. Exemple de référence nouvelle partie :

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
runtime). Signalé au worker du parcours avec l'état et le comparateur ; non corrigé ici.

Pendant l'ouverture (`M01M12`, 1.1.1) les mêmes 7 drapeaux de base sont déjà posés au premier
état (vblank 5217) et `1450`, puis `1451`, apparaissent pendant la scène : ce sont des
candidats « initialisation/scripts », pas des drapeaux de piste.

### Limites

- La création du personnage est pilotée au temps réel (A, OK tactile) : la séquence est
  reproductible mais pas à l'image près ; les points de contrôle sont lus en RAM, jamais
  déduits d'une capture d'écran.
- Le A en boucle ne marche pas : au-delà du premier contrôle, atteindre Angel Falls exige le
  **vrai chemin physique** (escaliers, portes, PNJ à qui parler). Les entrées du parcours réel
  du worker field seront rejouées dans melonDS ; aucun téléport ne servira de référence.
- Le combat 26 est un vrai combat (pas de contournement), mais le runtime le substitue : les
  drapeaux posés par le combat/le moteur peuvent manquer (c'est l'écart ci-dessus).
- Aucun quartet de quête à ces points ; les checkpoints « quêtes du village » et « fin de
  prologue » attendent le chemin physique.
- La sauvegarde d'état passe par le menu (environ 2 s, temps réel) ; ce sont des points
  d'histoire stables, pas des numéros d'image alignés.

## Suite

- Rejouer dans melonDS les entrées du parcours réel (premier contrôle → Angel Falls → quêtes
  → fin du prologue) et comparer chaque checkpoint ; puis le critère J2 « même nombre
  d'images » demande un pilotage image par image (pause + `HK_FrameStep`) que ce script ne
  fait pas encore.
- L'état contient aussi l'ARM7 et le son (`SPU.`) : hors périmètre tant que l'audio n'est pas
  porté.
