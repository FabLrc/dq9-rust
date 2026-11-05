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

## Suite

- Comparer les structures que `dq9-core` calculera (position, drapeaux d'événements via le
  gestionnaire de quêtes) à celles de la RAM : c'est le test différentiel de `architecture.md`.
- L'état contient aussi l'ARM7 et le son (`SPU.`) : hors périmètre tant que l'audio n'est pas
  porté.
