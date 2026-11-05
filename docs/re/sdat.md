# SDAT : musique (séquenceur SSEQ, banques, ondes)

Statut : le conteneur, les SSEQ/SBNK/SWAR de `bgm.sdat` et le choix de la séquence jouée par
une carte sont lus et vérifiés sur la ROM européenne (`YDQP`). Le séquenceur de `dq9-audio` est
un port logiciel du pilote son de la DS, écrit d'après la documentation publiée ; **il n'a pas
été comparé à l'oreille ni à melonDS** (voir « Non établi »).

Code : `crates/dq9-formats/src/sdat.rs` (conteneur et fichiers), `bgm.rs` (quelle séquence pour
quelle carte), `crates/dq9-audio` (séquenceur et mixeur), `crates/dq9-app/src/audio.rs`
(sortie), `cargo dq9re sseq` (rendu hors ligne).

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

## Sortie dans `dq9-app`

- `audio.rs` : au chargement d'une carte, `Audio::set_map` choisit la séquence (heure de la
  phase du jour, drapeaux d'histoire de l'état des triggers), charge la chanson (cache de 6) et
  la confie au moteur ; une séquence déjà jouée n'est pas relancée ; un changement fait
  disparaître l'ancienne en 0,4 s. Le moteur (`Engine`) est derrière un `Mutex` et produit le
  mixage dans le rappel du périphérique, au taux du périphérique.
- Natif : `cpal` (flux ouvert au premier besoin ; `--mute` n'ouvre aucun périphérique ; `M`
  coupe et rétablit). Web : `AudioContext` + `ScriptProcessorNode` créés et repris au premier
  geste (clavier, souris) ; `?mute=1`.

## Non établi

- **Écoute et comparaison** : aucune comparaison à l'oreille ni à une capture melonDS ; les
  mesures ci-dessus sont des propriétés objectives (hauteur, durée, niveaux).
- **Note de longueur 0** : tenue jusqu'à la fin de la séquence. `BG_100` et `BG_101` n'ont de sens
  qu'ainsi (note `30 7f 00` suivie de `C4` qui balaye la hauteur) ; la sémantique du SDK pour
  une longueur 0 n'a pas été relue dans son code **[hypothèse]**.
- Priorités des voix (`C6`), canaux matériels (16 ; PSG sur 8-13, bruit sur 14-15 non reproduits :
  32 voix quelconques), fondus du gestionnaire (`0x0209c2f0`, volumes 0..4 du réglage du joueur,
  table `{0, 0.378, 0.669, 0.850, 1.0}` à `0x020e8ec4`), jingles `ME_` déclenchés par texte ou
  victoire, effets sonores (`se_norm.sdat`, `se_btl.sdat`, des SSAR), flux STRM : non faits.
- Portamento et balayage : durées prises sur la longueur de la note quand la durée est 0 ; la
  formule exacte du SDK en pas de pilote n'est pas vérifiée **[hypothèse]**.
- Les 25 maps de fente 80 donnent `BG_100` ; la fente 25 (sans fichier, 2 cartes) donne le
  silence, comme dans la table.
