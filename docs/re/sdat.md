# SDAT : musique, jingles et effets sonores (séquenceur SSEQ, archives SSAR, banques, ondes)

Statut : le conteneur, les SSEQ/SBNK/SWAR de `bgm.sdat`, les archives de séquences (SSAR) des
effets de `se_norm.sdat`/`se_btl.sdat`, les joueurs et les groupes, le choix de la séquence jouée
par une carte et les natives de son des scripts sont lus et vérifiés sur la ROM européenne
(`YDQP`). Le séquenceur de `dq9-audio` est un port logiciel du pilote son de la DS, écrit
d'après la documentation publiée ; **il n'a pas été comparé à l'oreille ni à melonDS** (voir
« Non établi »).

Code : `crates/dq9-formats/src/sdat.rs` (conteneur et fichiers), `bgm.rs` (quelle séquence pour
quelle carte), `crates/dq9-audio` (séquenceur, mixeur multi-séquences, WAV),
`crates/dq9-app/src/audio.rs` (gestionnaire de son), `cargo dq9re sseq|sdat|se` (rendu hors
ligne et inspection).

## Sources

Dans l'ordre de `docs/ressources-externes.md` :

- page [SDAT](https://github.com/DQIX/wiki/blob/main/SDAT.md) du wiki DQIX : conteneur, SYMB,
  INFO, FAT, SSEQ, SBNK, SWAR, banques et ondes de `bgm.sdat` (sans licence : lue, pas copiée) ;
- GBATEK « DS Sound » (formats et ADPCM) ; spécification Nitro Studio 2 de Gota7 (jeu de
  commandes SSEQ) ;
- [SSEQPlayer](https://github.com/kode54/SSEQPlayer) (kode54) et [FSS](https://github.com/fincs/FSS)
  (fincs), tous deux WTFPL : tables de conversion (décibels, attaque, chute) et sémantique des
  commandes, adaptées avec mention dans `tables.rs` et `player.rs` ;
- la bibliothèque son de Nintendo liée au jeu est de la plate-forme : remplacée, pas traduite.

## Ce que nous avons vérifié sur la ROM

`crates/dq9-formats/tests/real_sdat.rs` (ignoré sans ROM) :

| contrôle | résultat |
|---|---|
| `data/sound/bgm.sdat` s'analyse ; séquences avec fichier | 64 |
| chaîne séquence → banque → archives d'ondes | 64 / 64 se résolvent |
| notes PCM des banques (toutes ADPCM IMA) | 2 079, toutes sur une onde existante qui se décode |
| `mapbgm.bin` | 67 entrées (l'enregistrement de tête en annonce 67) |
| musique de chaque carte du `maplist9.bin`, 4 heures du jour, drapeaux éteints/allumés | toujours une séquence qui a un fichier, ou aucune |

`crates/dq9-audio/tests/real_rom.rs` : chacune des 64 séquences produit, sur ses 10 premières
secondes, des échantillons finis, sous la pleine échelle, et n'est pas silencieuse.

## Le séquenceur

### Temps

Le pilote tourne **192 fois par seconde** ; chaque passage ajoute le tempo (BPM) à un compteur
et joue un *tick* chaque fois qu'il atteint 240. Un tick vaut 1/48 de noire : à 120 BPM, 96
ticks par seconde. Les durées de note, les attentes et les glissandos se comptent en ticks ; les
enveloppes et les modulations se mettent à jour à chacun des 192 passages. Mesuré par test
(`tempo_sets_the_note_length`) : une noire dure 0,500 s à 120 BPM et 0,250 s à 240.

### Commandes (octet de commande, arguments)

Tous les arguments multi-octets sont en petit-boutiste, sauf la longueur variable (7 bits par
octet, poids fort d'abord, bit 7 = suite). Les décalages de saut comptent depuis le début du flux
(`0x18` du fichier, soit `0x1C` sur toutes les séquences de la ROM).

| commande | rôle |
|---|---|
| `00..7F n v d` | note de touche `n`, vélocité `v`, durée `d` (longueur variable) ; si « attente de note » est active la piste attend `d` ticks. **`d = 0` : la note est tenue** jusqu'à la fin de la séquence [hypothèse, voir plus bas] |
| `80 d` | silence |
| `81 p` | programme (instrument de la banque) |
| `93 t o₂₄` | ouvre la piste `t` au décalage `o` |
| `94 o₂₄` / `95 o₂₄` / `FD` | saut / appel / retour (8 niveaux) |
| `D4 n` / `FC` | début de boucle (`n` passages, 0 = infini) / fin de boucle |
| `FE m₁₆` | masque des pistes utilisées (ignoré : les pistes s'ouvrent par `93`) |
| `FF` | fin de la piste |
| `A0 min₁₆ max₁₆`, `A1 v`, `A2` | préfixes : dernier argument tiré au hasard, lu dans une variable ; `A2` n'exécute la commande suivante que si la condition est vraie |
| `B0..B6 v x₁₆` | variable : =, +, −, ×, ÷, décalage, aléatoire ; `B8..BD` : comparaisons (==, ≥, >, ≤, <, ≠) qui posent la condition de `A2` |
| `C0` / `C1` / `D5` | pan (0..127, 64 = centre) / volume de piste / expression |
| `C2` | volume général de la séquence |
| `C3` / `C4` / `C5` | transposition / pitch bend (signés) / amplitude du bend (demi-tons) |
| `C7` / `C8` | attente de note / liaison (notes liées sur une même voix) |
| `C9` / `CE` / `CF` | touche de portamento / portamento actif / durée du portamento |
| `CA..CD`, `E0` | modulation : profondeur, vitesse, type (0 hauteur, 1 volume, 2 pan), amplitude, délai |
| `D0..D3` | attaque, chute, maintien, relâche qui remplacent ceux de l'instrument |
| `E1` | tempo (BPM, 16 bits) |
| `E3` | balayage de hauteur |
| `C6`, `D6` | priorité, affichage de variable : lus, sans effet |

Une commande inconnue arrête la piste ; une piste qui exécuterait 100 000 commandes dans un seul
tick sans attendre est arrêtée (garde contre les boucles sans attente). Un flux aléatoire ne
panique ni ne bloque (`garbage_streams_end_without_panic`).

### Voix, enveloppe, volume

- Une voix par note, 32 au plus ; au-delà on reprend la plus silencieuse en relâche, sinon la
  plus ancienne. L'instrument vient de la banque : note unique, jeu de percussions (une note par
  touche) ou répartition par plages de touches (`Instrument::note`).
- **Enveloppe** (unités 1/128 de 1/10 dB, de −723 × 128 à 0) : attaque `a ← a × taux / 256` avec
  `taux = 255 − attaque` (table pour attaque ≥ 109), chute `a −= chute` jusqu'au niveau de
  maintien, relâche `a −= relâche` jusqu'à −723 × 128 où la voix meurt. Chute et relâche :
  `127 → 0xFFFF`, `126 → 0x3C00`, `< 50 → 2v + 1`, sinon `0x1E00 / (126 − v)`.
- **Volume** : somme en 1/10 dB des `decibel_square` de la vélocité, du volume de piste, de
  l'expression, du volume de la séquence (enregistrement INFO) et du volume général, plus
  l'enveloppe et la modulation ; le gain linéaire est `10^(dB/200)`. `decibel_square(v)` =
  `400·log10(v/127)` avec le plancher −722 (la table du SDK : −722, −721, −651, −601, −562…
  retrouvés par test). **Pan** linéaire, centre à −6 dB sur chaque côté.
- **Hauteur** : en 1/64 de demi-ton autour de la touche de base de l'onde ; pitch bend,
  balayage et modulation s'y ajoutent. Mesuré par test : une onde de 32 échantillons à 32 728,5 Hz
  sonne à 1 022,8 Hz sur sa touche de base et exactement une octave plus haut 12 touches
  au-dessus (`notes_sound_at_their_pitch`).
- **Ondes** : IMA-ADPCM décodé une fois par `Song::load` (toutes les ondes de la banque), lu en
  interpolation linéaire, rebouclé au point de boucle. **PSG** : carré à 8 pas par cycle,
  rapport cyclique 1/8 à 7/8, fréquence de la touche 60 = `16756991 / (8 × 0x1F46)` ;
  **bruit** : registre à décalage 15 bits cadencé comme le PSG.
- **Mixage** : les voix s'additionnent en `f32` stéréo au taux de l'hôte ; les gains de chaque
  voix sont rampés linéairement entre deux mises à jour du pilote (pas de clic). La sortie
  reçoit un gain de marge de 0,6 (−4,4 dB) puis un limiteur doux à partir de 0,8.

### Rendu hors ligne

```powershell
cargo dq9re sseq                                   # liste les séquences de bgm.sdat
cargo dq9re sseq 5 --wav build/audio/bg005.wav     # intro + une boucle + 2 s de fondu
cargo dq9re sseq --map 1100 --time 0 --wav build/audio/af-night.wav
```

Mesures (release, 32 728 Hz, une passe de boucle puis 2 s de fondu) :

| séquence | tempo | pistes | octets de commandes | boucle atteinte à | durée du WAV | crête du mixage* | crête écrite | RMS | échantillons à pleine échelle |
|---|---|---|---|---|---|---|---|---|---|
| `BG_005` (Angel Falls, jour) | 90 BPM | 13 | 20 848 | 80,35 s | 82,35 s | 1,519 | 0,901 | 0,133 | 0 |
| `BG_006` (Angel Falls, nuit) | 70 BPM | 13 | 7 656 | 58,32 s | 60,32 s | 0,820 | 0,492 | 0,095 | 0 |

\* Avant gain de marge, à l'échelle du mixeur de la DS (1,0 = écrêtage matériel). Sur l'ensemble
des séquences essayées, de 0,35 (`ME_005`) à 2,04 (`BG_007`) : la DS écrête sur les plus fortes,
ce que le gain de marge et le limiteur évitent ici. L'équivalence exacte de nos gains avec le
mélange matériel (diviseurs de volume des canaux) est **[hypothèse]**.

## Quelle séquence joue sur une carte

Trouvé dans le code ARM9 (Ghidra, adresses européennes) :

1. `0x0209cd60` (`Bgm_SequenceForMap`) reçoit l'id de carte. Cas particulier des ids
   5900 à 5999 : si l'octet `+0x2788` de l'objet de zone vaut 0 et que son demi-mot `+0x2786`
   vaut 1900, l'id devient 1900 : **non modélisé** **[inconnu]** (ce que sont ces champs n'est
   pas lu).
2. Il lit l'enregistrement de la carte (`0x02099960`, voir `maplist.md`) : `+0xD` bits 0-6, la
   **fente musique** (slot 6), passe par `0x0209cb00` (`Bgm_SlotToSequence`).
3. `0x0209cb00` : fente ≥ 47 → −1 ; fente → `g_BgmSlotTable` (`0x020e8ee8`, 47 octets signés, vérifiés
   sur la ROM : l'identité sauf 0, 9, 25, 39 et 42 qui valent −1) ; **la nuit** (heure du jour 0
   dans `LightingManager+0x98`) 3 devient 4 et 5 devient 6 ; la fente 14 (régions de champ)
   devient 16 sous une condition globale (`0x0202ae28`/`0x0202b7e8`, objet non identifié)
   **[inconnu]** : `dq9-formats` garde 14.
4. Enfin `0x0209cc4c` (`Bgm_FindOverride`) parcourt `data/bin/mapbgm.bin` : une entrée est un mot
   de 32 bits (id de carte bits 0-15, séquence bits 16-21, drapeau d'histoire bits 22-30,
   bit 31 « de nuit seulement ») ; la première entrée de la carte dont le drapeau d'histoire est
   levé remplace la séquence (0 = silence). Totalement réimplémenté : `bgm::Overrides::find`.
5. Le chargeur `0x0209c3c4` (`Bgm_Select`) reçoit cet id tel quel : rien si c'est déjà la
   séquence en cours, sinon il arrête l'ancienne et charge la nouvelle. L'id est l'indice dans
   la liste des séquences de l'INFO de `bgm.sdat`, comme le wiki le déduisait de `maplist9.bin`
   (ici confirmé par le code, plus seulement par les valeurs). La native 736
   (`ov_001::02163aec`) enchaîne ces deux appels pour le lieu courant.

Angel Falls : la carte 1100 (`M01`, fente 5) joue `BG_005` de jour et `BG_006` de nuit ; son
église (1106, fente 10) `BG_010` ; la région autour (fente 14) `BG_014`. L'application prend
l'heure `--time`/`--clock` (nuit par défaut : `BG_006`).

Ce qui n'est pas lu : l'appel du chargeur depuis le champ (`ov_017`, trois appels à
`0x0209c3c4` en `0x0219efe4`, `0x021b19c8`, `0x021bcd34`) n'a pas été décompilé : l'application
choisit la séquence à chaque chargement de carte, ce qui est l'effet attendu de 736, sans avoir
prouvé que le jeu l'appelle au même instant.

## Archives, joueurs et groupes (`se_norm.sdat`, `se_btl.sdat`)

`dq9re sdat <bgm|se_norm|se_btl> [liste [indice]]` liste les huit listes d'enregistrements de
l'INFO avec leurs noms (`Sdat::record`, `Sdat::name`). Relevé sur la ROM (cases / remplies) :

| liste | `bgm.sdat` | `se_norm.sdat` | `se_btl.sdat` |
|---|---|---|---|
| séquences (SSEQ) | 82 / 64 | 0 | 0 |
| archives de séquences (SSAR) | 0 | 462 / 279 | 936 / 481 |
| banques | 82 / 60 | 462 / 279 | 936 / 481 |
| archives d'ondes | 81 / 59 | 108 / 3 | 105 / 5 |
| joueurs | 8 / 7 | 16 / 6 (numéros 10 à 15) | 17 / 5 (10 à 13 et 16) |
| groupes | 82 / 64 | 462 / 279 | 936 / 481 |

- **Enregistrement de séquence** (12 octets) : `+0` fichier, `+4` banque, `+6` volume, `+7`
  priorité des canaux, `+8` priorité du joueur, `+9` **numéro du joueur**. Le wiki DQIX les
  décale d'un cran (« pression polyphonique » en `+8`, « priorité du joueur » en `+9`) ; sur
  la ROM `+9` vaut 2 pour `BG_001` et 0 pour `BG_002` à `BG_005`, et `+8` vaut 0 : seul le
  décalage du SDK (`SNDSeqParam` : `cpr`, `ppr`, `ply`) donne des numéros de joueur existants
  (10 à 15 pour tous les effets). Corrigé dans `SequenceInfo` (`channel_priority`,
  `player_priority`, `player`).
- **Joueur** (8 octets) : `+0` nombre de séquences simultanées (u16), `+2` masque des canaux
  matériels (u16), `+4` tas (u32). `se_norm.sdat` : `PLAYER_NM` 4 séquences canaux 0-3,
  `PLAYER_FT` 1 (0-3), `PLAYER_SR` 8 (canaux 4-15), `PLAYER_NN` 1, `PLAYER_EV` 4, `PLAYER_PO`
  1 ; `bgm.sdat` : `PLAYER_BG12` 1 séquence (canaux 4-15), `BG16` 2 (0-15), `BG14` 2 (2-15),
  `BGR` 1, `ME12` 1, `ME16` 2, `ME14` 2. Les numéros des joueurs de `bgm.sdat` (0 à 7) et de
  `se_norm.sdat` (10 à 15) ne se recoupent pas : ils forment un seul espace de numérotation.
- **Groupe** : un compte puis des entrées de 8 octets `{type u8, indicateurs u8, 0, 0, indice
  u32}` ; types 0 séquence, 1 banque, 2 archive d'ondes, 3 archive de séquences, indicateurs
  bit 0 le fichier, bit 1 la banque, bit 2 les ondes. Le groupe `n` de `se_norm.sdat` charge la
  banque `n` et l'archive de séquences `n` (vérifié sur `GRP_SY_001` = 100 et `GRP_SP_ev05121` =
  278) : la native 726 appelée avec un seul argument joue donc l'archive de même numéro.
- **Archive de séquences (SSAR)** : `0x18` début du flux, `0x1C` nombre d'entrées, puis des
  entrées de 12 octets `{décalage u32, banque u16, volume u8, priorité des canaux u8, priorité
  du joueur u8, joueur u8, 0, 0}` ; un décalage `0xFFFFFFFF` marque une case vide. Les
  décalages de piste et de saut se comptent depuis le début du flux (même repère que les
  entrées). `Ssar::parse`. L'archive de base du champ, `se_norm.sdat` n° 100, a 205 cases dont 54
  remplies (banque 100 pour toutes, priorité des canaux 96, priorité du joueur 64 sauf l'effet
  152 à 54, joueurs 10 à 15) ; ses effets 200 à 204 sont ceux des portes (voir plus bas).
- **Notes de longueur 0** : les effets s'écrivent `81 p  n 7f 00  ff` (programme, touche,
  vélocité, longueur 0, fin de piste). Avec « tenue jusqu'à la fin de la séquence puis
  relâchée » ces effets s'éteignaient aussitôt (rendu muet de l'effet 12, 10, 18, 200…). Le
  séquenceur laisse donc jouer jusqu'au bout les notes tenues dont l'onde ne boucle pas ; une
  onde qui boucle est relâchée à la fin des pistes **[hypothèse]**. Sur la ROM les 54 effets de
  l'archive 100 sonnent et finissent d'eux-mêmes en moins de 20 s (test
  `field_effects_sound_and_end`), les mesurés durent de 0,05 à 0,65 s.

## Gestionnaire de son du jeu (natives 712 à 738)

Le gestionnaire est le singleton `0x02108760`. Champs lus dans `Se_*` et `Bgm_*`
(`re/symbols.txt`, section « Sound ») : `+0xb4` archive de base (100 sur le terrain, 101 en
combat), `+0xbc` archive chargée par 730, `+0xc4` archive chargée par 726, `+0xc8` bits de
verrou (bit 2 : native 735), `+0xc9` état du jingle, `+0xce` séquence du jingle.

| native | rôle | reproduit par |
|---|---|---|
| 726 / 730 `(groupe[, archive])` | charge le groupe puis choisit l'archive (par défaut le numéro du groupe) à `+0xc4` / `+0xbc` (`Se_LoadGroup` `0x0205eb64`, `Se_LoadSecondaryGroup` `0x0205eae0`) | `Sound::LoadEffects` : l'archive est décodée (banque et ondes) et mise en cache |
| 727 / 731 | rend le tas de DS | sans effet : les archives décodées restent en cache |
| 728 / 732 `(indice[, &poignée])` | joue l'effet `indice` de l'archive `+0xc4` / `+0xbc` (`Se_PlayLoaded` `0x0205eba0`, `Se_PlaySecondary` `0x0205eb34`) sur une des 16 poignées des scripts (ou sur celle du gestionnaire sans adresse) | `Sound::PlayEffect`, `handle` −1 sans adresse |
| 712 `(indice)` | joue sur l'archive **de base** (`Se_PlayBase` `0x0205eab0`) ; avec une adresse, même chose que 728 (`ov_001::02163564`) | `Sound::PlayBaseEffect` / `PlayEffect` |
| 729 / 723 `(poignée, durée)`, 733 `(poignée)` | arrête l'effet, avec ou sans fondu | `Sound::StopEffect` |
| 713 `(séquence)` | `Bgm_Select` puis arrêt | `Sound::SelectBgm` |
| 714 | relance la séquence choisie au volume 127 (`Bgm_Restart` `0x0209c5f8`) | `Sound::RestartBgm` |
| 715 `(volume, durée)` | volume de la musique et des joueurs 5 à 7 (`Bgm_SetVolume` `0x0209c2f0`), durée en images **[hypothèse]** | `Sound::BgmVolume`, fondu linéaire |
| 721 `(durée)` | arrêt avec fondu (`Bgm_Stop` `0x0209c688`) | `Sound::StopBgm` |
| 720 `(séquence)`, 725 `(&occupé)` | **jingle** : voir ci-dessous ; 725 est vrai tant que le jingle n'a pas fini | `Sound::Jingle`, `Signal::JingleEnded` |
| 736 | `Bgm_SequenceForMap` du lieu puis `Bgm_Select` | `Sound::ZoneBgm` |
| 737 `(séquence)` | arrête la musique et joue la séquence seule, sans reprise (`Bgm_PlaySequence` `0x0209c6e8`) ; ids observés 67 (`ME_018`) et 55 (`ME_006`) | `Sound::PlaySequence` |
| 738 | remet tout à zéro, volume 127 (`Bgm_Reset` `0x0209c21c`) | `Sound::ResetBgm` |
| 735 `(activé)` | verrou : sélection, relance et arrêt sont sans effet | `Sound::BgmEnabled` |
| 734 `(mode)` | mode de mémoire audio (tas BGM `0x57000` / `0xa2000`, ouvre ou ferme l'archive de base) | sans équivalent (mémoire de la DS) |
| 554 `(voix)` | `W+0x99c` : voix du bip du texte (0, 1, 2) | `Sound::TextVoice` |

**Jingle** (720, `Bgm_UpdateJingle` `0x0209c850`, états 1 à 3 de `+0xc9`) : à la demande la
musique descend à 0 en 20 images ; après 800 ms elle est mise en pause (non arrêtée) et la
séquence du jingle démarre ; quand elle a fini, 500 ms plus tard la musique reprend avec un
fondu de 30 images et `+0xc9` revient à 0 (c'est là que 725 passe à faux). `Audio::step`
refait cette suite (48, 30 et 30 images de 1/60 s). Muet ou sans périphérique, la demande
est terminée aussitôt (sinon le script attendrait pour toujours).

Sons des scripts du prologue (`dq9-app --stage 1.1 --headless-steps 30000 --press-every 20
--sound-wav build/audio/open.wav`, journal du son) : `ME_018` (id 67, native 737) pendant
`ev21507` ; des effets 0 à 20 des archives chargées par les groupes de chaque scène (278 pour
`ev01130`, effet 12) ; la musique `BG_005` relancée. Le WAV fait 123 s, crête 0,948. L'ordre et
les durées ne sont pas comparés à melonDS.

### Sons du champ (archive de base, `se_norm.sdat` n° 100)

Trouvés en décompilant les appelants de `Se_PlayBase` (`0x0205eab0`, une centaine de sites) :

| indice | quand | où |
|---|---|---|
| 10, 12, 11 | bip d'un caractère tapé, selon la voix (0, 1, 2) ; coupé quand `+0x19b2` vaut 0 (native 413, « texte muet » d'après le wiki) | `MsgWin_PlayTextBlip` `0x02045608`, appelée par `MsgWin_Update` quand le compteur `+0x912` dépasse 3 pendant la frappe : un bip tous les 4 pas **[hypothèse : un caractère par pas]** |
| 6, 28 | `<EXC>` (`0xFF2E`) et `<QES>` (`0xFF2F`) ; le jeu fixe ensuite `+0x1959` à 60 (bulle, attente probable) : **attente non reproduite** | `MsgWin_Update` `0x020659a0` |
| 5 | ouverture d'un choix (`<YESNO>`, `<UKEYAME>`…) | `MsgWin_Update` |
| 1 | réponse à un choix : première et seconde réponses pour `0xFF04`/`0xFF05`/`0xFF08` ; aucune pour `<YESNO_NOTSE>` (`0xFF06`) ; première seulement pour `<YESNO_NOTSE_IIE>` (`0xFF07`) ; B (annulation, −2) est silencieux | `MsgWin_AnswerYesNo` `0x020678cc` (`0xFF04`..`0xFF07`) et `MsgWin_AnswerAcceptDecline` `0x020679e4` (`0xFF08`), appelés par la table `0x020f08b8` de `MsgWin_UpdatePrompt` `0x020676f0` |
| 101 | `<ST=…>` (`0xFF1C`) avec un choix valide **[hypothèse]** | `MsgWin_Update`, non branché |
| 14 | code `0xFF4B` (non nommé) | `MsgWin_Update`, non branché |
| 200 à 204 | porte d'une région : `g_DoorSeTable[indicateurs & 0xF]` (`0x020e6e84`) | `Region_Fire` `0x02018310` **[hypothèse : le rôle de porte]** ; non branché (le passage de porte de l'application ne passe pas par cette fonction) |
| 18, 19 (`0x12`, `0x13`) | ouverture et fermeture d'une porte animée (wiki, `func_02015554`) | non branché : la tâche de porte n'existe pas encore |
| 50 (`0x32`) | nuage d'un PNJ qui disparaît | `0x020486a0`, non branché |
| 96 (`0x60`) | changement d'état d'un objet du champ | `0x02034054`, non branché |

**Choix d'un message** : `MsgWin_UpdatePrompt` (`0x020676f0`) rend 0 (A sur la première
réponse), 1 (A sur la seconde) ou −2 (B), puis appelle le gestionnaire du type de choix par la
table statique `0x020f08b8` (deux entrées : `0x020678cc` pour `<YESNO>`, `<NOYES>`,
`<YESNO_NOTSE>` et `<YESNO_NOTSE_IIE>` ; `0x020679e4` pour `<UKEYAME>`). Le premier lit le
code du prompt dans le flux (`0xFF04` à `0xFF07`) et joue l'effet 1 de l'archive de base :
`0xFF06` ne joue pour aucune réponse ; `0xFF07` joue pour la première et reste silencieux pour
la seconde ; `0xFF04` et `0xFF05` jouent pour les deux. Le second (`<UKEYAME>`) joue
toujours. Les deux sautent le bloc sonore pour B (−2), qui répond « non » sans effet. La case
1 de l'archive 100 est bien remplie (décalage `0x144` du flux, banque 100, volume 61). Le
déplacement haut/bas (`0x0205cedc` → `0x0205c580` → `0x0205bb94`, ou `0x02045750` quand
`+0x19bc` est posé) ne joue aucun effet : aucun appelant de `Se_PlayBase` dans ce chemin.

**Pas** : `Object3D::AdvanceAnimations` (`0x02034ea0`) joue l'effet dont l'indice figure dans
une liste liée de l'animation (`animation + 0x20`), sur l'archive `objet + 0x7a`, quand
l'image dépasse l'image de la note : ce sont les bruits de pas et de gestes, portés par les
données d'animation. Ni le format de cette liste ni l'archive de chaque personnage ne sont lus
**[inconnu]** : les pas ne sont pas reproduits.

Ces effets se rendent par `dq9re se <indice> [--archive 100]` ; mesures (32 728 Hz, crête avant
limiteur) : 5 (0,645 s, 0,327), 6 (0,609 s, 0,452), 10 (0,056 s, 0,183), 12 (0,073 s, 0,299),
20 (0,151 s, 0,380), 28 (0,340 s, 0,325), 152 (0,071 s, 0,007).

## Sortie dans `dq9-app`

- `audio.rs` : au chargement d'une carte, `Audio::set_map` choisit la séquence (heure de la
  phase du jour, drapeaux d'histoire de l'état des triggers), charge la chanson (cache de 6) et
  la confie au mixeur ; une séquence déjà jouée n'est pas relancée ; un changement fait
  disparaître l'ancienne en 0,4 s. Le moteur (`Engine`) est derrière un `Mutex` et produit le
  mixage (`dq9_audio::Mixer`) dans le rappel du périphérique, au taux du périphérique ; si
  celui-ci diffère de 48 kHz, le mixeur est refait et la musique relancée à l'ouverture.
- `Mixer` : plusieurs `Player` à la fois (musique, jingle, effets), chacun sur le joueur de
  son enregistrement. Un joueur plein (`PLAYER` : séquences simultanées) laisse sa place à la
  séquence de priorité la plus faible, la plus ancienne à égalité ; si toutes celles qui jouent
  ont une priorité strictement plus forte, la nouvelle est refusée **[hypothèse : la règle du
  SDK n'est pas relue]**. Les canaux matériels (masques) ne sont pas modélisés.
- Événements : `EventPlayer` rend `Outcome::Sound` ; `Audio::script_sound` l'exécute. Le texte
  des messages (`dq9_core::talk::Sound`) passe par `Talk::take_sounds` puis `Audio::talk_sound`.
  Chaque pas fixe appelle `Audio::step` (attentes du jingle).
- `--sound-wav f.wav` avec `--headless-steps N` : le son est mixé hors ligne à 32 728 Hz
  (aucun périphérique) et écrit en WAV ; le journal liste chaque effet de script.
- Natif : `cpal` (flux ouvert au premier besoin ; `--mute` n'ouvre aucun périphérique ; `M`
  coupe et rétablit). Web : `AudioContext` + `ScriptProcessorNode` créés et repris au premier
  geste (clavier, souris) ; `?mute=1`.

## Non établi

- **Écoute et comparaison** : aucune comparaison à l'oreille ni à une capture melonDS ; les
  mesures ci-dessus sont des propriétés objectives (hauteur, durée, niveaux).
- **Note de longueur 0** : tenue jusqu'à la fin de la séquence pour une onde qui boucle, jusqu'à
  la fin de l'onde sinon. `BG_100` et `BG_101` n'ont de sens qu'ainsi (note `30 7f 00` suivie
  de `C4` qui balaye la hauteur) ; la sémantique du SDK pour une longueur 0 n'a pas été relue
  dans son code **[hypothèse]**.
- Canaux matériels (16 ; PSG sur 8-13, bruit sur 14-15 non reproduits : 32 voix quelconques par
  séquence), priorité des canaux (`C6`, `+7` des enregistrements), règle exacte de remplacement
  d'un joueur plein, réglage du joueur (volumes 0..4, table `{0, 0.378, 0.669, 0.850, 1.0}` à
  `0x020e8ec4` : on joue au maximum), unité des durées des fondus (prises en images de 1/60 s),
  flux STRM, effets de combat (`se_btl.sdat`, archive de base 101).
- Sons du champ non branchés : portes, bulles `<EXC>`/`<QES>` (attente de 60 images), pas et
  gestes (listes de sons des animations). La validation d'un choix est branchée (effet 1) ;
  le déplacement du curseur est silencieux d'après l'absence d'appelant de `Se_PlayBase` dans
  son chemin, sans écoute pour le confirmer.
- Portamento et balayage : durées prises sur la longueur de la note quand la durée est 0 ; la
  formule exacte du SDK en pas de pilote n'est pas vérifiée **[hypothèse]**.
- Les 25 maps de fente 80 donnent `BG_100` ; la fente 25 (sans fichier, 2 cartes) donne le
  silence, comme dans la table.
