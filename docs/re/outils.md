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
