# Oracle melonDS : sauvegardes d'état (`.ml1`)

Module `dq9_formats::melonds`, commande `dq9re state <fichier>` (sans ROM).

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
| `0x020f33d8` | instance de `GameState` : la valeur est le mot du littéral lu par `GameState::GetInstance` (`0x0200f398`) | ARM9 européen |
| `GameState+0x5cb0 / 4 / 8` | chapitre, étape, sous-étape (`Save_Chapter`, `Save_ScenarioStep`, `Save_ScenarioSubstep`) | `re/symbols.txt` |

`State::read(addr, len)` adresse la RAM principale (miroir tous les 4 Mio), le DTCM à
`0x027e0000` et l'ITCM (miroir tous les 32 Kio) ; une lecture ne traverse pas deux mémoires.

## Utilisation

```powershell
cargo dq9re state partie.ml1                       # sections, VBlank, chapitre/étape/sous-étape
cargo dq9re state partie.ml1 --peek 020f33d8:40    # octets en hexadécimal (adresse:longueur)
```

Pour produire un état : dans melonDS, *Système → Enregistrer l'état* (fichiers `.ml1`
à côté de la ROM, un par emplacement).

## Vérification

**[non vérifié sur un état réel]** : aucun fichier `.ml1` n'a encore été lu. Les tests
(`melonds::tests`) utilisent un état synthétique construit selon ce format. Premier contrôle à
faire : charger un état d'une partie dont on connaît le chapitre et comparer `scenario` à ce
que montre le jeu (et à la sauvegarde `.sav` correspondante, `dq9re save`). Si l'adresse de
`GameState` ou les décalages des sections diffèrent, corriger ici ; `State::read` est
déjà borné.

## Suite

- Contrôler sur un état réel (ci-dessus), puis ajouter la comparaison des champs que
  `dq9-core` calculera (position, drapeaux d'événements via le gestionnaire de quêtes) à ceux
  de la RAM : c'est le test différentiel de `architecture.md`.
- L'état contient aussi l'ARM7 et le son (`SPU.`) : hors périmètre tant que l'audio n'est pas
  porté.
