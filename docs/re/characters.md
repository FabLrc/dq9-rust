# Personnages : la distribution d'une carte, les archives `.chr` et les tables `.bcfg`

Implémentation : `crates/dq9-formats/src/cast.rs` (`load`, `run_placements`, `Story`,
`Placement`, `Npc::appearance`, `initial_motion`, `sprite_stand`) et
`crates/dq9-formats/src/chr.rs` (`Chr`, `MotionTable`, `role`, `Motion::advance`) ; le héros
(pièces, assemblage, allures) : § « Le héros ». Lecture de la
RAM : `melonds::State::npcs` et `State::story`. Outils :

```powershell
cargo dq9re cast M01                                   # prologue (1.1.1, jour)
cargo dq9re cast M01 --chapter 2 --night --quest 4=3   # autre moment de l'histoire
cargo dq9re cast C01M09 --state build/melonds/demo.ml1 # comparé à la table des PNJ en RAM
cargo dq9re chr data/chara_sub/s017.chr                # rôles des membres, mouvements
```

Sources : code ARM9 et `ov_017` (adresses européennes ci-dessous), décompilation USA
(`src/Resource/BCFG.cpp`, `src/World/Object3D.cpp`, Unlicense), wiki DQIX (pages Area-Cast,
Motion-Tables, Sprites, Character-Parts : hypothèses revérifiées dans le code). Les
enregistrements de `place.bin` y sont appelés « tags » ; ici « enregistrements de type n »
(`table.md`).

## Chaîne de chargement

Après l'assemblage de la carte (`map-archive.md`), `ov_017::021a2c14` (`Cast_LoadMap`) :

1. ouvre `data/scenario/%s.npc` (NARC), `%s` = trois premiers caractères du code de la carte,
   ou `F` seul pour les cartes `F*` (`cast::archive_path`) ;
2. exécute chaque membre dont le nom contient `place.bin` avec `Place_Run` (`0x0206da90`) :
   (chapitre, étape, sous-étape) de la piste courante (`Save_Chapter`…), identifiant de la
   carte ; chaque exécution **repart d'une liste vide** (seul le dernier compte ; il n'y en a
   qu'un par archive) ;
3. exécute chaque membre `npc.bin` avec `Cast_RunNpcBin` (`0x02064e3c`) ; sur les cartes dont
   le nom contient `R0` sauf `R05`, `0x02064e98` ajoute des visiteurs tirés du bloc de zone
   de la sauvegarde (**non modélisé**) ;
4. construit la table des PNJ (`NpcTable_Build`, `0x0203d038`), puis restaure les positions
   gardées pour cette carte (`0x0203e2d4`, liste `+0x9e` de la table, **non modélisé**) ;
5. pose sur le sol chaque PNJ sans drapeau `0x800` ni `0x1000` : `Zone3D_GroundHeight`
   (`0x02018fcc`) lance un rayon de `y + 1` à `y − 10` dans un carré de ±0,5 et rend la hauteur
   touchée + 0,1 (`0x199`). Sans collision de carte ici, `cast` rend la hauteur du fichier.

La table des PNJ (`g_NpcTable`, `0x021075f4`, rendue par `0x0203cf5c`) : `+0xc` 32 pointeurs
d'entrées, `+0x8c` nombre, `+0x90` liste de distribution, `+0x94` liste des placements.

## `<zone>npc.bin` : la distribution

Table d'enregistrements ; gestionnaires `{1, 2 : rien ; 3 : Cast_RecMember}`
(`g_CastHandlers`, `0x020f05f8`). Type 3 (`0x02064d58`), nœud de `0x14` octets :

| Champ | Nœud | Sens |
|---|---|---|
| 0 | `+0` u16 | identifiant du PNJ (fichiers de dialogue `NNN_<lg>.bin`, placements) |
| 1 | `+2` | genre : 0 sprite, 1 rien (chose à examiner, `dialogue.md`), 2 modèle, 5 monstre ; 4 = visiteur (posé par `0x02064e98`, absent des données) |
| 2 | — | nom japonais, lu puis jeté |
| 3 | `+4` (12 octets, `strcpy`) | nom de la ressource (`n003a`, `s017`, `z015d`) |
| 4 | `+3` | **[inconnu]** (0, 1 ou 2) |

Un enregistrement n'est gardé que si le PNJ a une chaîne de placements (voir plus bas), et pas
pour les PNJ 1 à 63 des cartes 50101 à 50523 (`0x020981f4`, `0x02098278` : visiteurs). Les
nœuds sont **ajoutés en tête** : la table des PNJ est dans l'ordre inverse du fichier.

## `<zone>place.bin` : le script de placement

Table d'enregistrements dont chaque type est un opcode (`g_PlaceHandlers`, `0x020f0994`,
types 3 à 22). Contexte `g_PlaceContext` (`0x02108cec`) : `+0` sous-étape, `+1` étape,
`+2` chapitre, `+4`/`+8` dernières quêtes des types 21/14, `+6` dernier PNJ des types 14/21,
`+0xa` **compteur** (incrémenté par les types 3, 4, 5, 14, 21, et 17 s'il a au moins 9
champs), `+0xc` carte, `+0x10` liste. `Place_Run` remet `+4`, `+6`, `+8` à −1 et le compteur à
0 avant chaque exécution sauf la toute première de la partie (valeurs nulles du BSS : effets
identiques, vérifié cas par cas).

**Champs persistants.** Le lecteur de tables (`Tbl_Next`, `0x020307e0`) décode les champs d'un
enregistrement dans un tableau qu'il ne vide jamais : un enregistrement court qui lit au-delà
de ses champs lit **ceux des enregistrements précédents**. Exemples : une zone de conversation
à cinq champs prend l'étiquette du champ 5 d'avant ; un segment de type 8 à deux champs, la
longueur d'avant. Reproduit par `table::Fields` ; aucun enregistrement des 508 cartes ne lit
un champ jamais écrit (test `casts_resolve_to_existing_archives`).

### Enregistrements qui placent

Conversions : flottant × 4096 tronqué (`fx32`, `Script::Parameter::ToVec3fix`) ; l'angle en
radians × 4096 est gardé sur 16 bits (`+0x1c`). « Temps » : 0 de jour seulement, 1 de nuit
seulement, autre valeur toujours (`Save_IsDaytime`, `0x020103c4` : sauvegarde `+0x3dc` ≠ 0).
Une progression vaut `chapitre × 1000 + étape × 10 + sous-étape`.

| Type | Fonction | Champs | Effet |
|---|---|---|---|
| 3 bloc | `0x0206c020` | carte, PNJ, x, y, z, angle, (mouvement) | si la carte est celle-ci : moins de 3 champs → retire le PNJ, sinon place ; une autre carte ne fait **rien** |
| 4 point | `0x0206c108` | chapitre, étape, sous-étape, temps, carte, PNJ, x… | compte si les trois valent la progression et le temps convient ; autre carte → **retire** le PNJ ; moins de 7 champs → retire ; sinon place |
| 5 intervalle | `0x0206c2d0` | de (3), à (3), temps, carte, PNJ, x… | compte si `de ≤ maintenant ≤ à` et le temps convient ; autre carte → retire ; moins de 10 champs → retire ; sinon place |
| 14 quête | `0x0206cbdc` | quête, condition, temps, carte, PNJ, x… | voir « Quêtes » ; moins de 6 champs → retire |
| 17 drapeaux | `0x0206d4f0` | (drapeau, levé)…, temps, carte, PNJ, x…, (mouvement si nombre pair) | moins de 9 champs : ignoré ; chaque paire : poids fort 1 → `Flags_Test(n)`, 2 → `Flags_TestExt(n)` (n ≥ `0x400` décalé de `0x6fa`), autre → ignoré ; le temps n'est pas gardé |
| 21 quête et intervalle | `0x0206cf68` | quête, condition, de (3), à (3), temps, carte, PNJ, x… | quête comme 14, puis intervalle et temps ; moins de 12 champs → retire |

Le champ optionnel après l'angle (`+0x1e`) est l'**indice du mouvement initial** du modèle
(voir plus bas). Retirer (`Place_Remove`, `0x0206dd78`) détache toute la chaîne du PNJ : un
enregistrement suivant peut le replacer.

### Chaînes et priorités (`Place_Insert`, `0x0206db58`)

Chaque PNJ a une chaîne de nœuds de `0x78` octets (`Place_InitNode`, `0x0206bf3c`) ; sa tête
est l'endroit où il se tient. Classe d'un nœud, la plus basse en tête :

| Classe | Nœud |
|---|---|
| 0 | type 17 (`+0xa` bit 2) |
| 1 | type 21 (`+0xa` bit 6) |
| 2 | type 14 (`+0x46` ≥ 0) |
| 3 | étape de départ non nulle et étape de fin nulle : type 4, ou intervalle « ouvert » |
| 4 | autre intervalle |
| 5 | bloc |

À classe égale, les classes 1 et 4 mettent devant le **début le plus tôt** (octets `+4..+6`),
puis l'enregistrement le plus ancien ; les autres, l'enregistrement le plus ancien. Un nœud qui
ne passe devant aucun autre est **abandonné** (il ne serait jamais en tête). Le nœud garde la
valeur du compteur (`+0x44`) : c'est ainsi que les enregistrements suivants le retrouvent.

### Quêtes (types 14 et 21)

Condition (champ 1) sur le quartet de la quête (gestionnaire de quêtes `+0x2cc`, `0xcc`
quêtes, `Quest_State` = 2 bits bas) : 0 en cours (état 2), 1 bit 2, 2 terminée (3), 3 proposée
(1), 4 bit 3 et état 0, −1 état 0 ; autre valeur : toujours vrai.

Pour un même PNJ (le compteur `+6` change avec le PNJ et remet `+4`/`+8` à −1), une quête
terminée est **ignorée** si une quête terminée de numéro supérieur a déjà été traitée par un
enregistrement du même type ; et tout type 14 est ignoré après un type 21 réussi de quête > 0.
Exemple : C01M09, PNJ 10, quête 4 terminée → le nœud de type 14 passe devant le bloc.

### Enregistrements qui complètent le dernier placement

Ils cherchent, dans la chaîne du PNJ du champ 0, le nœud fait au compteur courant ; sans lui,
rien.

| Type | Fonction | Champs | Nœud |
|---|---|---|---|
| 6 | `0x0206c4f8` | PNJ, f1–f4, étiquette | zone de conversation ajoutée à `+0x40` : x et z max, x et z min (× 4096) ; `Talk_NearestNpc` (`dialogue.md`) |
| 7 | `0x0206c624` | PNJ, x max, z max, x min, z min | zone `+0x24..+0x30`, mode `+0x1f` = 7 **[hypothèse : zone d'errance]** |
| 8 | `0x0206c704` | PNJ, direction, distance | mode 8, `+0x22` = direction, zone = position ± distance·(sin, cos) remise en ordre max/min **[hypothèse : va-et-vient]** |
| 9, 10 | `0x0206c83c`, `0x0206c948` | PNJ, (x, y, z)… | chemin bouclé `+0x34`, mode 9 / 10 ; le 9 écrit aussi le nombre de points − 1 dans l'octet bas de `+0x22` |
| 11 | `0x0206ca4c` | PNJ, valeurs… | OU dans les drapeaux `+0xc` (64 et 4096 dans les données) |
| 12 | `0x0206cae8` | PNJ, PNJ | `+0xb` **[inconnu : PNJ lié]** |
| 13 | `0x0206cb58` | PNJ, f | `+0x38` = f × 4096 (`0x7a` par défaut), donné à l'objet par `0x02040c0c` **[hypothèse : vitesse]** |
| 15 | `0x0206d3e4` | PNJ, angle | `+0x3c`, drapeau `0x800` : PNJ **assis** (voir sprites) |
| 16 | `0x0206d474` | PNJ, x, y, z | `+0x48`, drapeau `0x2000` : objet `data/chara_sub/s069.chr` partagé, à l'échelle des PNJ **[inconnu]** |
| 18 | `0x0206d744` | PNJ, n, (f) | `+0xa` bits 3–5 = n, `+0x60` = f × 4096 **[inconnu]** |
| 19 | `0x0206d7f4` | PNJ, archive, mouvement | `+0x54` chemin sous `data/`, `+0x58` nom de mouvement (voir « Mouvement initial ») ; une archive nulle fait copier le jeu depuis l'adresse 0, chargement qui échoue ensuite (équivalent à « aucune ») |
| 20 | `0x0206d8f8` | PNJ, f1, f2, f3 | `+0x5c` = (f2, 0, f3, f1) × 4096 **[inconnu]** |
| 22 | `0x0206d9e8` | PNJ, r, g, b | `+0x20` = r \| g≪5 \| b≪10, `+0xa` bit 7 : couleur **ajoutée** (saturée) à la palette du modèle (`0x0203db54`) |

## Ce qui est dessiné (`NpcTable_Build`, `0x0203d038`)

Une entrée de `0x20` octets par PNJ de la distribution, dans l'ordre de la liste : `+0` drapeaux
(ceux du placement y sont ajoutés, `0x02040550`), `+8` nœud de distribution, `+0xc` nœud de
placement, `+0x10` objet `s069`, `+0x14` sprite, `+0x18` modèle, `+0x1c` objet de jeu.

| Genre | Corps |
|---|---|
| 2 | modèle `data/chara_sub/<nom>.chr`, `Object3D::LoadFromCCHROrCMOTArchive` (`0x02036814`) |
| 5 | monstre : membre `<nom>.mon` de `data/pack_lv5/enemy.gp2` (NARC), son premier membre `.cchr` (LZ10) chargé comme un modèle |
| 4 | visiteur (personnage de l'équipe d'un autre joueur) |
| autre | objet sprite ; feuille chargée seulement si le genre est 0 et le nom non vide : `<nom>_s.spr` si le placement a le drapeau `0x800` et que `data/ani/<nom>_s.spr` existe, sinon `<nom>.spr` ; le chargeur ajoute `data/ani/` (`0x0203fcf0`) |

Puis, avec le placement : position (`0x02040784` : sprite `+4`, `Object3D +0x44`), rotation
`(0, angle, 0)` (`0x0204086c` : sprite `+0x14`, `Object3D +0x50..+0x58`), échelle des modèles
`0x10a` (≈ 0,065), mouvement initial, teinte (type 22), sprite assis (`+0x90` = angle du
type 15).

**Unités.** Positions et angles sont dans les unités des objets du monde (`fx32`, radians ×
4096), les mêmes que la position du héros (`Object3D +0x44`) : dans `demo.ml1`, héros et PNJ de
l'église sont dans le même repère. Chaque intérieur est une carte à part, avec sa propre
origine.

### Mouvement initial des modèles

1. `NpcModel_SetMotionIndex` (`0x020415a8`) → `Object3D::MaybeSetBCFGAnimation`
   (`0x020370b0`) avec l'octet `+0x1e` du placement (0 par défaut) sur la table de l'archive
   du PNJ (premier paquet) ; le temps part du début de l'enregistrement.
2. Si le type 19 nomme un mouvement : `Object3D::MaybeSetRegularAnimation` (`0x02036e44`)
   cherche ce nom dans les paquets dans l'ordre (le sien, puis celui du type 19, ajouté en
   queue) et ne retient qu'un mouvement qui a des fichiers d'animation ; le temps part de 0.

`cast::initial_motion` rend `(paquet, indice)`. Exemple : `s017` dans M01 au prologue :
`stand`.

### Sprites : direction affichée

`NpcSprite_FaceCamera` (`0x0203cbe8`) choisit chaque image l'animation d'après l'angle du PNJ
moins celui de la caméra (`0x0202e800`), ramené dans [0, 2π[ : huit secteurs centrés sur les
multiples de π/4 (`FX_Div((a + 2π)·8 + π, 2π) >> 12 & 7`), table `0x020e7948` vers les noms de
`0x020efd00` : `stand_down`, `stand_r_down`, `stand_right`, `stand_r_up`, `stand_up`,
`stand_l_up`, `stand_left`, `stand_l_down` (`cast::sprite_stand`). Les PNJ assis
(drapeau 8 du sprite) ajoutent l'angle du type 15 et lisent une autre table (indices 16–20 :
`sleep_*`) ; d'autres modes utilisent quatre secteurs (`0x020e7928`, `0x020e7930`)
**[non modélisé]**. Le format `.spr` est décrit par le wiki (Sprites) **[non implémenté ici]**.

## Archives `.chr`

NARC non compressés (2 683 dans la ROM). Rôle d'un membre d'après son nom (`strstr`,
`chr::role`) :

| Membre | Rôle |
|---|---|
| contient `.nsbmd` | modèle (seul le premier compte) |
| contient `.bcfg` | table de mouvements (seule la première compte) |
| `<mouvement>.nsbca/.nsbma/.nsbtp/.nsbta` | animations du mouvement |
| `.beff`, `.bact` | scripts d'effets et d'actions (lus ailleurs) |

Deux chargeurs :

- **par mouvement** (`LoadFromCCHROrCMOTArchive`, `0x02036814`, PNJ et monstres) : pour chaque
  mouvement de la table, les membres de nom exact `<nom>.<ext>` dans l'ordre nsbca, nsbma,
  nsbtp, nsbta (`object3DsData.extensionLookup`) ; un mouvement sans fichier ne joue rien
  (`Chr::motions`) ;
- **à quatre emplacements** (`InternalLoadFromCHRArchive`, `0x0203644c`, effets et objets) :
  tout membre dont le nom contient une extension d'animation remplit l'emplacement de son
  genre, le dernier gagne (`Chr::slots`).

### Lecture d'un mouvement (paquet par mouvement)

`Object3D::AdvanceAnimations_v1` (`0x02035188`) : le temps (`fx32`, en images du fichier)
avance de `vitesse × delta × vitesse de l'objet` (produits arrondis) et boucle sur
`[0, nombre d'images − 1[` ; `delta` = 4096 par 17 ms de jeu
(`GameState::CalculateDeltaTime`, `0x02010160`). Les bornes `début`/`fin` de la table ne
servent qu'aux paquets à quatre emplacements (`AdvanceAnimations_v0`) : dans les `.chr` de PNJ
elles désignent des tranches d'une ancienne animation unique (`walk` 10–18, `step_left` 10–14…)
**[hypothèse]**. `Motion::advance`.

## Tables `.bcfg` (`BCFG::LoadFromScript`, `0x0203462c`)

Table d'enregistrements exécutée avec `bcfgScriptOpcodes` (décompilation, `BCFG.cpp`) :

| Type | Champs | Effet |
|---|---|---|
| `0x64` | n | réserve n mouvements (la table repart de zéro) |
| `0x66` | nom, début, fin, vitesse | ajoute un mouvement s'il reste de la place (valeurs × 4096 ; nom copié sur 16 octets) |
| `0x6a` / `0x6c` | n / mouvement, temps, valeur | réserve n marques ; ajoute une marque **en tête** de la liste du mouvement **[inconnu]** |
| `0x6d` / `0x6f` | n / mouvement, temps, son | idem pour les signaux joués quand le temps normalisé franchit la valeur (0 : au départ et à chaque boucle, 1 : à la fin) |
| `0x71` | os | os racine (`waist` si absent) |
| `0x65`, `0x6b`, `0x6e` | — | rien |
| autres (`0x67`–`0x69`, `0x70`) | — | sans gestionnaire : ignorés |

Les dépassements de capacité et les mouvements inconnus sont ignorés comme dans le jeu.

## Le héros (`dq9_formats::hero`, `dq9_render::hero`)

Implémentation : `crates/dq9-formats/src/hero.rs` (`HeroLook`, `ItemModels`, `Outfit`),
`crates/dq9-render/src/hero.rs` (assemblage, `Gait`), `AnimatedScene::add_hero`. Outil :

```powershell
cargo dq9re hero                                   # héros d'une nouvelle partie : parts résolues
cargo dq9re hero --save rom/dq9.sav                # héros d'une sauvegarde (--character N pour un autre)
cargo run -p dq9-app --release -- --map M01 --frame 30 --walk "-x:5" --start=-1.74,1.22 `
    --eye=-1.5,0,-3 --target=-3,-0.6,1.2 --screenshot build/scratch/app/hero.png
```

Sources : wiki DQIX (Character-Parts, Character-Presets, Single-Questions, Motion-Tables,
Weapon-Positions : tout y est lu sur la ROM européenne, revérifié ci-dessous), code ARM9 pour
les tables d'animation, RAM de melonDS pour l'échelle.

### Un personnage humain est un jeu de pièces

Les dix emplacements d'équipement h0–h9 d'un personnage (`CharaParts_GetPartNumbers`, USA
`func_02072afc` ; h2 le visage, h3 les cheveux) sont des **objets**. Chaque objet nomme un
modèle de `data/pack_lv5/chara_pc.gp2` par son identifiant (`item_part`) : la tranche de mille
donne la lettre (12 `m` couvre-chef, 13 `b` armure, 14 `a` bras, 15 `g` gants, 16 `p` jambes,
17 `r` chaussures, 20 `w` arme, 21 `s` bouclier), le reste le numéro à trois chiffres : 13007 →
`p_b007`. Les 14 identifiants de présélection sans pièce (wiki) existent aussi dans les
sauvegardes (13309 → `p_b309`, absent) : le jeu les rattache à la pièce d'un autre objet
**[inconnu]** ; nous retombons sur les sous-vêtements `p_p090`.

- **Sauvegarde** (`save.md`, `Character::equipment`, `face_item`, `hair_item`) : visage `+356` et
  cheveux `+358` sont des identifiants d'objets 9020–9033 et 9000–9013 (sauvegarde réelle :
  9027 et 9008) ; équipement `+352..+370` ; octet `+372` bit 0 le sexe (0 homme, 1 femme,
  d'après les objets réservés à un sexe, wiki), bits 1–3 le teint, 4–7 les yeux ; `+373`
  couleur des cheveux ; `+376`/`+378` la corpulence.
- **Visage et cheveux** : `itemdt_def.nat` (membre de `data/prm/itemdt_def.gp2`, 32 octets par
  enregistrement, l'identifiant à `+4`) ; le mot **8 octets avant l'identifiant** donne le
  modèle (`func_020de234`, wiki) : bits 0–9 numéro d'un homme, 10–19 d'une femme (999 : celui de
  l'autre sexe), bits 20–27 la lettre (`h`, `f`). Vérifié sur les 28 objets pour les deux sexes :
  chaque pièce existe (`hero_parts_resolve`). Visage 9024 = `p_f004` (homme), `p_f014`
  (femme) ; cheveux 9006 = `p_h060` (homme), `p_h180` (femme).
- **Forme des cheveux** (`func_02072e94`, wiki) : `a` sans couvre-chef, sinon le numéro du
  couvre-chef ÷ 100 indexe `"bbdcc\0cea\0"` ; les centaines 5, 9 et dix et plus **ne dessinent
  pas de cheveux** ; un homme aux cheveux 9001 sous un 3xx prend `f`. Modèle `p_h<nnn><forme>`,
  texture de couleur `p_h<nn><couleur>a.nsbtx` (texture `p_h<nn>0a_00` liée par nom ; la
  couleur est la couleur de cheveux de la sauvegarde **[hypothèse]**, wiki : « INFERRED »),
  couleur 0 à défaut.
- **Bras, gants, chaussures** sont des **textures** : le corps `p_b…` lie un matériau à
  `p_a000_00`, les jambes `p_p…` un autre à `p_r000_00` ; le fichier chargé décide
  (`p_g<nnn>.nsbtx` si des gants sont portés, sinon `p_a<numéro de l'armure>.nsbtx` ;
  chaussures `p_r<nnn>.nsbtx`). L'assemblage place ces fichiers **avant** les textures du
  modèle dans la recherche par nom (`TextureSources`).

### Le gabarit et l'assemblage

Corps et jambes portent le même squelette de 14 os (`root`, pièce, `waist`, `chest`,
`arm0L/1L/0R/1R`, `head`, `usiro`, `leg0L/1L/0R/1R`) ; les pistes de l'animation sont lues par
**indice d'objet** (`JointAnimation::transforms`), donc la même animation pose les deux. Visage,
cheveux et couvre-chef n'ont qu'un os à eux : ils sont posés au repos puis déplacés par la
matrice de l'os `head` du corps, celle que tiennent ses commandes de rendu après
`MulObject(head)` (`object_worlds` rejoue la pile de matrices). Vérifié : le visage et les
cheveux tombent sur le cou (taille totale ≈ 21 unités du modèle, test
`hero_model_follows_its_gait`). Les pièces sont fusionnées en un seul modèle de scène.

**Échelle.** Objet du héros en RAM (`demo.ml1`, `Object3D+0x5c`, fix16 ×3) : `(0xec, 0x109,
0xec)` pour des mots de corpulence `0xe39` et `0xffb` dans la sauvegarde : `0x10a × mot / 4096`
tronqué, sur x et z pour la largeur, y pour la hauteur (même `0x10a` que les PNJ). Rotation de
l'objet : angle du jeu (0 regarde +z), `dq9_core::field::angle_of_yaw`.

### Mouvements

Le jeu choisit l'animation d'un personnage par état (`Character_UpdateAnimation`,
`0x02033de4`) : état `+0xbe`, type `+0xc0` ; le type suivant est `g_CharaStateKinds[état × 13 +
type]` (`0x020ef9c4`, lu en RAM) et `Character_PlayKindAnimation` (`0x02033bb0`) joue le nom
`g_CharaKindNames[type]` (`0x020ef994`) ou, au type 0, `g_CharaIdleNames` (`0x020ef978`) selon
l'expression (`+0xc1 >> 4`). Résultat : **repos (état 0) → `stand`, marche au clavier
(état 1) → `run`** ; le jeu ne joue `walk` que par script (natives 210…). Les noms viennent des
paquets `mp%02d%02d` (corps `02`, jeu de mouvements de l'arme `00` sans arme, wiki) :

| Allure | Paquet (`chara_pc.gp2`) | Mouvement | Vitesse (`.bcfg`) |
|---|---|---|---|
| repos | `mp0200n` | `stand` | 0,1 |
| marche | `mp0200ne` | `walk` | 0,1 |
| course | `mp0200n` | `run` | 0,4 |

`stand` existe dans trois paquets (`n`, `f`, `n2`) : nous prenons `n`, celui qui bouge le moins
le personnage (wiki) **[hypothèse]** ; lecture « par mouvement » (`AdvanceAnimations_v1`,
boucle sur `[0, images − 1[`). **Choix de l'allure dans l'application** (`Gait::from_speed`) :
vitesse horizontale mesurée sur le pas (unités/s) : repos sous 0,05, marche jusqu'à
`RUN_SPEED` = 5, course au-delà **[hypothèse]** : le jeu joue `run` à toute vitesse, `walk`
sert ici aux vitesses sous la vitesse maximale établie (5,76 unités/s, `movement.md`), donc à
la vitesse provisoire de 4 unités/s de `dq9-core` et à l'accélération d'un déplacement
fidèle. À reprendre quand le déplacement fidèle sera là.

### Apparence par défaut (nouvelle partie)

`HeroLook::new_game` : homme, visage 9024, cheveux 9006 (présélections masculines de
`charapreset.bin`), teint 3, tenue de céleste du début d'Angel Falls **[hypothèse]** :
costume céleste 13007, bas célestes 16215, souliers célestes 17120 et auréole 12805 (la
création du personnage est en overlay 9 : le héros réel n'est pas modélisé). Avec `--save` :
le héros de l'emplacement courant (premier personnage du groupe).

### Non fait

- **Armes et boucliers** ne sont pas dessinés (`wpnpos.bin` : placements partiellement
  inférés, bouclier absent du fichier) ; les ailes (`d_hane`, gabarit à 21 os de
  `chara_pd.gp2`) non plus.
- **Teints, yeux, sourcils** : `data/chara/palette.bin` recolore les palettes du visage et des
  bras (Character-Colours) ; seules les palettes d'origine sont utilisées **[inconnu]**.
- Couleurs des cheveux : la rampe de peau de `GetItemSkinShades` (`0x020731f0`, wiki) n'est pas appliquée.
- Éclairage : lumière fixe des personnages (`app.md`), pas les lumières DS.
- Pièces d'objets sans modèle (voir plus haut) ; compagnons du groupe ; le héros pendant les
  scènes (`walk` des scripts, échelle et caméra des événements).

## Vérifications

- **RAM (melonDS)** : `cargo dq9re cast --state build/melonds/demo.ml1` (carte 109 `C01M09`,
  histoire 19.2.1, nuit, quête 4 terminée) : 2 PNJ, `n016a` (25, bloc) puis `n002a` (10, type
  14), identiques à la table en RAM : ordre, identifiant, genre, octet 3, nom, carte, position,
  angle, octet de mouvement, compteur (`0x3d`, `0x21`), quête, drapeaux ; position et angle des
  sprites aussi, sauf la hauteur posée sur le sol (0,350 → 0,349, 0,100 → 0,099). Même résultat
  sur `before.ml1`, `field.ml1`, `moved.ml1`.
- **ROM** (`crates/dq9-formats/tests/real_rom.rs`) :
  - `character_archives_read_by_role` : 2 683 `.chr` lus, 1 910 modèles, 2 511 tables,
    6 466 mouvements dont 3 782 avec fichiers ; dans `chara_sub`, 1 055 mouvements dont 626
    sans fichier (`step_*`) et 7 animations qu'aucun mouvement ne nomme ; aucun nom de plus de
    15 caractères ; 601 monstres (`.cchr` de `enemy.gp2`) avec modèle et table ;
  - `casts_resolve_to_existing_archives` : 508 cartes avec une archive `.npc`, au début de
    chacun des 19 chapitres, de jour et de nuit (19 304 distributions, 36 170 PNJ : 21 167
    sprites, 2 511 modèles, 370 monstres, 12 122 sans corps) : chaque feuille, archive ou
    membre existe et se lit (238 distincts) ; aucune lecture de champ jamais écrit ; C01M09 à
    l'état de `demo.ml1` donne les nœuds vus en RAM.
- **Héros** : `hero_parts_resolve` (`dq9-formats`, 28 objets de visage et de cheveux × 2 sexes
  résolus en pièces existantes, tenue de départ complète) et `hero_model_follows_its_gait`
  (`dq9-render`, modèle assemblé, déterministe, mouvement selon l'allure) ; échelle comparée à
  la RAM de `demo.ml1` (voir « Le héros »).

## Inconnu

- Champ 4 de `npc.bin` (`+3`) ; types 12, 16, 18, 20 ; sens exact des drapeaux `+0xc` (64,
  4096) et des modes 7–10 (déplacements : `ov_017`, non lu).
- Le comportement des PNJ après le chargement (errance, chemins, regard vers le héros) et ce
  qui change ensuite leur mouvement (`stand`/`walk`).
- Les visiteurs des cartes `R0*` (`0x02064e98`) et les positions restaurées (`0x0203e2d4`).
- Le format `.spr` (wiki Sprites) et l'angle de caméra que soustrait `0x0203cbe8`.
