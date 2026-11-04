# Architecture cible du portage

Approche : **réimplémentation** en Rust (comme OpenMW ou les ports issus de décompilation),
pas un émulateur. La logique du jeu est réécrite à partir de l'analyse du code ARM9 ; les
ressources sont lues **à l'exécution depuis la ROM du joueur** et ne sont jamais
redistribuées.

```mermaid
flowchart TD
    rom[nds-rom<br/>conteneur NDS, LZ/BLZ, NARC] --> formats[dq9-formats<br/>gp2, modèles/textures Nitro, SDAT, tables]
    formats --> core[dq9-core<br/>état, règles, VM de scripts<br/>déterministe, sans E/S]
    core --> render[dq9-render<br/>wgpu]
    core --> audio[dq9-audio<br/>séquenceur SDAT]
    render --> app[dq9-app<br/>winit : Windows, macOS, Linux, Web]
    audio --> app
    re[dq9re<br/>outillage RE] --> rom
```

État : `nds-rom`, `dq9-formats` (compression Level-5, GPC2) et `dq9re` existent. Les autres
crates seront créées quand le format
ou le sous-système correspondant aura été documenté dans `docs/re/`.

## Règles

- **Portabilité** : `nds-rom`, `dq9-formats` et `dq9-core` ne font aucune E/S et compilent
  pour `wasm32-unknown-unknown` (vérifié en CI). Seule `dq9-app` touche au système.
- **Déterminisme** : `dq9-core` avance par pas fixes (60 Hz comme la DS) à partir d'entrées
  explicites, ce qui permet les replays et la comparaison avec l'émulateur.
- **Données non fiables** : tout parseur renvoie une erreur sur une donnée corrompue, jamais
  de panique.
- **Traçabilité** : chaque comportement réimplémenté cite la fonction d'origine
  (`// ov_017::0218c0f4`) et le format correspondant est décrit dans `docs/re/`.

## Vérification

1. Tests unitaires sur données synthétiques (formats, cas limites).
2. Tests sur la vraie ROM (`crates/*/tests/real_rom.rs`), ignorés automatiquement sans ROM.
3. Comparaison différentielle avec melonDS (stub GDB) : même entrée → même état mémoire
   des structures réimplémentées.
