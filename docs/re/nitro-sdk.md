# NitroSDK ARM9 : premier et second lots vérifiés (86 routines)

Ces **86 routines de `Module::Main` européen** sont classées **Plateforme**, non
portées : elles seront remplacées, pas traduites. Aucun code SDK ou ROM n'est
reproduit ici. Les compteurs de fonctions traduites n'augmentent pas.

## Sources et portée

- [DQIX épinglé](https://github.com/DQIX/dqix-decomp/tree/b5c617aa3113a57baba07465ede3be7a0be5328f) : noms source et correspondances EUR de `re/functions.txt`.
- [NitroSDK de référence épinglé](https://github.com/ntrtwl/NitroSDK/tree/ccbbcc8b6db4c4dbba3d503d2026978d78977840) : les chemins et lignes du tableau sont relatifs à cette référence.
- [Wrapper pret/pokeheartgold épinglé](https://github.com/pret/pokeheartgold/tree/e1d5a4606640e31c44d36714ad0b5cdd9c8763fd) et sa dépendance NitroSDK : provenance de la référence, pas preuve binaire DQ9.

DQ9 utilise **4.2.7539**, la référence **4.2.30001**. L'identification est
**fonctionnelle/structurelle**, jamais un byte-match interversions. Chaque routine
a été examinée individuellement : **D** = décompilation européenne, **I** =
instructions, **P** = pools lus dans l'ARM9 EUR. Le tableau associe ces observations
au fichier de référence et à un marqueur structurel ; les fichiers temporaires
ignorés sous `build/` ne sont pas la seule preuve publiée.

La whitelist exige simultanément **module Main + adresse EUR + taille + nom source
exact**. `re/functions.txt` conserve le nom original ; `re/symbols.txt` consigne
le canonique sans inventer de mode ARM/Thumb. Les suffixes **`_`** sont conservés :
ils désignent les implémentations, contrairement aux wrappers publics inline.
Aucune extension par préfixe, voisinage ou appel : overlays, ITCM, helpers CP et
autres callees ne sont pas absorbés. Les helpers de
[`ExtendedVectorMath.cpp`](https://github.com/DQIX/dqix-decomp/blob/b5c617aa3113a57baba07465ede3be7a0be5328f/src/Graphics/ExtendedVectorMath.cpp#L1-L146)
restent **Jeu** ; composer des appels SDK ou partager un préfixe ne prouve pas
l'identité d'une routine SDK autonome. Les anonymes voisins restent non identifiés.

## Correspondances individuelles

| Adresse EUR | Taille | Nom source → canonique SDK | Référence fichier:lignes | Preuve structurelle |
|---|---:|---|---|---|
| `0x020c112c` | `0xc` | `Mat2x2_WriteRotation` → `MTX_Rot22_` | `libraries/fx/src/fx_mtx22.c:44-52` | D/I — cosinus aux deux positions diagonales, sinus puis sinus négatif hors diagonale. |
| `0x020c1138` | `0x58` | `Mat2x2_ApplyScale` → `MTX_ScaleApply22` | `libraries/fx/src/fx_mtx22.c:56-70` | D — Quatre produits signés 64 bits, facteur X pour les deux premiers éléments, Y pour les deux suivants ; réduction Q12 sans ajout d’arrondi. |
| `0x020c1190` | `0x24` | `Mat3x3_WriteIdentity` → `MTX_Identity33_` | `libraries/fx/src/fx_mtx33.c:12-23` | D — Cet ordre particulier concorde avec l’assembleur source. |
| `0x020c11b4` | `0xc0` | `Mat3x3_ApplyScale` → `MTX_ScaleApply33` | `libraries/fx/src/fx_mtx33.c:87-108` | D — Aucun arrondi ajouté. |
| `0x020c1274` | `0x1c` | `Mat3x3_WriteRotationX` → `MTX_RotX33_` | `libraries/fx/src/fx_mtx33.c:112-129` | D — Ordre des écritures concordant. |
| `0x020c1290` | `0x1c` | `Mat3x3_WriteRotationY` → `MTX_RotY33_` | `libraries/fx/src/fx_mtx33.c:133-150` | D — Cosinus aux indices 0/8 écrits en premier, sinus aux indices 6/2 avec signe opposé, unité à l’indice 4 ; mêmes zéros et ordre. |
| `0x020c12ac` | `0x18` | `Mat3x3_WriteRotationZ` → `MTX_RotZ33_` | `libraries/fx/src/fx_mtx33.c:154-167` | D — Bloc de rotation aux indices 0/1/3/4, signes concordants, indice 8 à `0x1000` ; mêmes positions nulles. |
| `0x020c12c4` | `0x2f0` | `Mat3x3_Invert` → `MTX_Inverse33` | `libraries/fx/src/fx_mtx33.c:212-274` | D — Temporaire de neuf mots si entrée=sortie ; trois cofacteurs et déterminant avec ajout `0x800` avant réduction Q12 ; retour −1 si déterminant nul ; réciproque asynchrone, autres cofacteurs calculés pendant l’attente, neuf résultats signés ; copie finale si temporaire, retour 0. |
| `0x020c15b4` | `0x220` | `Mat3x3_Multiply` → `MTX_Concat33` | `libraries/fx/src/fx_mtx33.c:276-331` | D — Temporaire seulement si sortie=second opérande ; neuf produits scalaires à trois termes, Q12 sans arrondi ; même ordre inhabituel des résultats `00,01,02,12,11,10,20,21,22`, réutilisation des colonnes, copie finale de neuf mots. |
| `0x020c17d4` | `0x7c` | `Mat3x3_ApplyToVector` → `MTX_MultVec33` | `libraries/fx/src/fx_mtx33.c:333-347` | D — Lecture préalable des trois coordonnées ; trois produits scalaires utilisant successivement les colonnes 0/3/6, 1/4/7, 2/5/8 ; réduction Q12, sans translation. |
| `0x020c1850` | `0x28` | `Mat4x3_WriteIdentity` → `MTX_Identity43_` | `libraries/fx/src/fx_mtx43.c:9-21` | D — diagonale 0/4/8 à `0x1000`, autres éléments nuls, dont les trois composantes de translation ; ordre concordant. |
| `0x020c1878` | `0x34` | `Mat4x3_ConvertTo4x4` → `MTX_Copy43To44_` | `libraries/fx/src/fx_mtx43.c:23-38` | D — Copie de quatre groupes de trois mots ; insertion successive de 0,0,0,`0x1000` dans la quatrième colonne ; mêmes regroupements et progression des pointeurs. |
| `0x020c18ac` | `0xac` | `Mat4x3_ApplyTranslation` → `MTX_TransApply43` | `libraries/fx/src/fx_mtx43.c:55-73` | D — Branche entrée≠sortie avec appel de copie du bloc linéaire ; trois produits scalaires Q12 de la translation demandée avec les colonnes de la matrice, ajoutés à la translation existante. |
| `0x020c1958` | `0x34` | `Mat4x3_ApplyScale` → `MTX_ScaleApply43` | `libraries/fx/src/fx_mtx43.c:94-101` | D — Appel direct à la routine 3×3 identifiée à `0x020c11b4`, puis copie inchangée des trois mots de translation aux offsets `0x24/0x28/0x2c`. |
| `0x020c198c` | `0x20` | `Mat4x3_WriteRotationX` → `MTX_RotX43_` | `libraries/fx/src/fx_mtx43.c:105-123` | D — Écriture du sinus et de son opposé en premier aux indices 5/7 ; unité à 0, cosinus à 4/8 ; translation nulle et mêmes autres zéros. |
| `0x020c19ac` | `0x1c` | `Mat4x3_WriteRotationY` → `MTX_RotY43_` | `libraries/fx/src/fx_mtx43.c:127-143` | D — Sinus à l’indice 6 écrit en premier, opposé à 2, cosinus à 0/8, unité à 4 ; translation nulle ; ordre concordant. |
| `0x020c19c8` | `0x1c` | `Mat4x3_WriteRotationZ` → `MTX_RotZ43_` | `libraries/fx/src/fx_mtx43.c:147-162` | D — Même bloc Z que la version 3×3, unité à 8 puis trois mots de translation nuls ; positions, signes et ordre concordants. |
| `0x020c19e4` | `0x38c` | `Mat4x3_Invert` → `MTX_Inverse43` | `libraries/fx/src/fx_mtx43.c:175-248` | D — Inversion du bloc linéaire par cofacteurs avec arrondis `0x800`, test du déterminant et réciproque asynchrone ; temporaire de douze mots si alias ; translation inverse par trois produits scalaires négativés ; copie finale et retours −1/0 concordants. |
| `0x020c1d70` | `0x2d4` | `Mat4x3_Multiply` → `MTX_Concat43` | `libraries/fx/src/fx_mtx43.c:250-317` | D — Même branche d’alias avec le second opérande ; neuf résultats du bloc linéaire, puis translation calculée dans l’ordre 32/31/30 avec ajout de celle du second opérande ; Q12 sans arrondi et copie finale de douze mots. |
| `0x020c2044` | `0xa0` | `Mat4x3_ApplyToVector` → `MTX_MultVec43` | `libraries/fx/src/fx_mtx43.c:319-339` | D — Trois coordonnées capturées avant écritures ; produits scalaires Q12 identiques à la version 3×3, puis ajout séparé de chaque composante de translation. |
| `0x020c20e4` | `0x108` | `Mat4x3_WriteViewMatrix` → `MTX_LookAt` | `libraries/fx/src/fx_mtx43.c:341-375` | D — Séquence complète concordante. |
| `0x020c21ec` | `0x2c` | `Mat4x4_WriteIdentity` → `MTX_Identity44_` | `libraries/fx/src/fx_mtx44.c:13-26` | D — Seize écritures avec unité Q12 aux indices 0/5/10/15 et zéros ailleurs, dans l’ordre de progression de l’assembleur source. |
| `0x020c2218` | `0x34` | `Mat4x4_ConvertTo4x3` → `MTX_Copy44To43_` | `libraries/fx/src/fx_mtx44.c:42-57` | D — Quatre copies successives de trois mots, en sautant systématiquement le quatrième mot de chaque ligne source ; mêmes groupements et progression. |
| `0x020c224c` | `0x664` | `Mat4x4_Multiply` → `MTX_Concat44` | `libraries/fx/src/fx_mtx44.c:281-360` | D — Temporaire de seize mots seulement si sortie=second opérande ; seize produits scalaires à quatre termes, Q12 sans arrondi ; ordre des sorties et réutilisation des colonnes concordants, dont première ligne 00/01/03/02 ; copie finale complète. |
| `0x020c28b0` | `0x14c` | `Mat4x4_MaybeWriteFrustum` → `MTX_PerspectiveW` | `libraries/fx/src/fx_mtx44.c:416-460` | D/I/P — Pool `0x020c29f8 = 0x04000290`. |
| `0x020c29fc` | `0x208` | `Mat4x4_WriteProjectionUnknown` → `MTX_OrthoW` | `libraries/fx/src/fx_mtx44.c:462-511` | D/I/P — Pool `0x020c2c00 = 0x04000290`. |
| `0x020c2c04` | `0x10` | `fix32_Divide` → `FX_Div` | `libraries/fx/src/fx_cp.c:14-18` | D — enchaînement de la mise en file du quotient puis de la récupération arrondie. |
| `0x020c2c14` | `0x34` | `fix32_Sqrt` → `FX_Sqrt` | `libraries/fx/src/fx_cp.c:38-48` | D+P — test du signe, chemin positif configurant SQRT en mode 64 bits, opérande `x<<32`, puis appel au lecteur FX. |
| `0x020c2c48` | `0x24` | `GetHardwareDividerResult` → `FX_GetDivResultFx64c` | `libraries/fx/src/fx_cp.c:69-72` | D+P — attente du busy `0x8000`, retour des deux mots du quotient sans arrondi/décalage. |
| `0x020c2c6c` | `0x38` | `fix32_GetDivisionResult` → `FX_GetDivResult` | `libraries/fx/src/fx_cp.c:74-77` | D+P — même attente DIV, ajout `0x80000` avec propagation de retenue, extraction après décalage de 20 bits. |
| `0x020c2ca4` | `0x30` | `fix32_QueueComputeReciprocal` → `FX_InvAsync` | `libraries/fx/src/fx_cp.c:79-85` | D+P — mode DIV `1`, numérateur 64 bits dont mots bas/haut sont `0 / 0x1000`, dénominateur argument puis mot haut nul ; aucune attente. |
| `0x020c2cd4` | `0x2c` | `fix32_GetSqrtResult` → `FX_GetSqrtResult` | `libraries/fx/src/fx_cp.c:107-110` | D+P — attente SQRT, lecture résultat, ajout `0x200`, décalage de 10 bits. |
| `0x020c2d00` | `0x28` | `fix32_QueueComputeQuotient` → `FX_DivAsync` | `libraries/fx/src/fx_cp.c:112-118` | D+P — mode DIV `1`, numérateur `numer<<32`, dénominateur 32 bits avec mot haut nul, retour immédiat. |
| `0x020c2d28` | `0x3c` | `FastIntDivide` → `FX_DivS32` | `libraries/fx/src/fx_cp.c:120-127` | D+P — mode DIV `0`, écriture des arguments, effacement haut du dénominateur, attente busy, lecture du quotient à `0x040002a0`. |
| `0x020c2d64` | `0x3c` | `FastIntModulus` → `FX_ModS32` | `libraries/fx/src/fx_cp.c:129-136` | D+P — préparation indépendante identique à la division entière, mais lecture du reste à `0x040002a8`. |
| `0x020c2da0` | `0x34` | `Vector3fix_Add` → `VEC_Add` | `libraries/fx/src/fx_vec.c:7-16` | D — trois additions 32 bits aux offsets `0,4,8`, écrites dans la destination ; structure et ordre des composantes concordants. |
| `0x020c2dd4` | `0x34` | `Vector3fix_Subtract` → `VEC_Subtract` | `libraries/fx/src/fx_vec.c:18-27` | D — trois soustractions `a-b` aux offsets `0,4,8`, sans conversion ni arrondi. |
| `0x020c2e08` | `0x3c` | `Vector3fix_InnerProduct` → `VEC_DotProduct` | `libraries/fx/src/fx_vec.c:53-59` | D — trois produits signés élargis, accumulation 64 bits, un seul ajout d’arrondi `0x800`, extraction décalée de 12 bits. |
| `0x020c2e44` | `0x84` | `Vector3fix_CrossProduct` → `VEC_CrossProduct` | `libraries/fx/src/fx_vec.c:73-88` | D — six produits signés élargis, trois différences cycliques, retenues/emprunts 64 bits, ajout `0x800` puis extraction Q12 pour chaque composante. |
| `0x020c2ec8` | `0x60` | `Vector3fix_Length` → `VEC_Mag` | `libraries/fx/src/fx_vec.c:112-130` | D+P — somme 64 bits des trois carrés, multiplication par quatre, SQRT64, attente, résultat arrondi par ajout de un puis division par deux. |
| `0x020c2f28` | `0x118` | `Vector3fix_Normalize` → `VEC_Normalize` | `libraries/fx/src/fx_vec.c:151-178` | D+P — somme des carrés ; DIV64/64 en mode `2` avec numérateur `2^56` ; SQRT64 de quatre fois cette somme ; récupération des deux résultats ; multiplication réciproque×racine ; trois mises à l’échelle avec arrondi correspondant à l’extraction de 45 bits. |
| `0x020c3040` | `0x7c` | `Vector3fix_Distance` → `VEC_Distance` | `libraries/fx/src/fx_vec.c:230-251` | D+P — trois différences de composantes 32 bits, somme de leurs carrés élargis, facteur quatre, SQRT64 et arrondi final comme `VEC_Distance`. |
| `0x020c339c` | `0x1b8` | `fix32_Atan2` → `FX_Atan2` | `libraries/fx/src/fx_atan.c:158-244` | D+P — même partition par quadrants, axes et diagonales ; constantes `±3217`, `±6434`, `±9651`, `±12868` ; division du plus petit module par le plus grand, index `>>5`, lecture signée 16 bits, ajout/soustraction selon quadrant, résultat tronqué signé 16 bits. |
| `0x020c7d44` | `0x4c` | `SaveContext` → `OS_SaveContext` | `libraries/os/src/os_context.c:68-96` | I+P — appel CP à contexte `+0x48`, sauvegarde CPSR, passage SVC `0xd3`, sauvegarde SP_SVC, restauration mode, sauvegarde `r0-r14` avec valeur de reprise `1`, PC+4 synthétique et retour initial `0`. |
| `0x020c7d90` | `0x44` | `RestoreContext` → `OS_LoadContext` | `libraries/os/src/os_context.c:100-130` | I+P — appel CP à `+0x48`, passage SVC masqué, chargement SPSR, SP_SVC et PC+4, restauration des registres avec suffixe `^`, instruction neutre, retour d’exception soustrayant quatre au PC sauvegardé. |
| `0x020ca5a4` | `0xbc` | `VectorizedMemset` → `MI_CpuFill8` | `libraries/mi/src/mi_memory.c:402-509` | D — variante non-small ; traitement destination impaire par conservation de l’octet adjacent, duplication de la valeur, alignement demi-mot puis mot, boucle 32 bits, queues 16/8 bits avec conservation de l’octet extérieur. |
| `0x020ca660` | `0x180` | `VectorizedInvertedMemcpy` → `MI_CpuCopy8` | `libraries/mi/src/mi_memory.c:526-755` | D — source puis destination ; amorce destination impaire, séparation suivant XOR d’alignement bit0 puis bit1, reconstruction de demi-mots si parités opposées, chemins demi-mot/mot, queue préservant l’octet voisin. |
| `0x020ca96c` | `0xb4` | `DecompressC` → `MI_ReadUncompRL8` | `libraries/mi/src/mi_uncomp_stream.c:43-102` | D+I — contexte destination/reste/flags/longueur aux offsets `0,4,0xb,0xc`, littéraux ou répétitions selon bit7, longueurs `+1` ou `+3`, arrêt sur épuisement d’entrée et sauvegarde du contexte. |
| `0x020caa20` | `0x184` | `DecompressA` → `MI_ReadUncompLZ8` | `libraries/mi/src/mi_uncomp_stream.c:173-295` | D+I — contexte aux offsets `0,4,8,0xf,0x10,0x11,0x12`, huit flags, reprise de longueur partielle, distinction format étendu, constantes `0x110/0x1110`, distance 12 bits augmentée de un, copie depuis la sortie antérieure. |
| `0x020caba4` | `0x16c` | `DecompressB` → `MI_ReadUncompHuffman` | `libraries/mi/src/mi_uncomp_stream.c:445-542` | D+I — mêmes neuf champs de contexte, chargement progressif de l’arbre, racine `context+0x1d`, navigation par déplacement six bits et bit de branche, marque terminale `0x80`, accumulateurs entrée/sortie, émission 32 bits. |

## Corrections et limites de compilation

- `Mat4x4_MaybeWriteFrustum` est **`MTX_PerspectiveW`**, pas `MTX_FrustumW` :
  sept paramètres, division cosinus/sinus et cotangente ; multiplication de cette
  cotangente par `scaleW` en **32 bits**, pas en 64 bits. La seconde projection est
  `MTX_OrthoW`. Les tailles incluent les mots de pool finaux.
- `DecompressC/A/B` sont les lecteurs **streaming RL8/LZ8/Huffman** de
  `mi_uncomp_stream.c`, non les décompresseurs monobloc de `mi_uncompress.c`.
  RL8 et LZ8 utilisent `SWPB`, branche sans `MI_USE_STRB`. Dans DQ9, l'ajout
  conditionnel RL8 et la mise à jour de racine/saturation Huffman sont développés
  en branches plutôt qu'en instructions conditionnelles : pas d'identité binaire.
- Les contextes correspondent à la variante **ARM9 avec `SDK_CONTEXT_HAS_SP_SVC`** ;
  `RestoreContext` reçoit le canonique **`OS_LoadContext`**.
- Les routines mémoire correspondent aux variantes **non-small** ; FX/VEC aux
  chemins de production sans assertions. Ces choix de build et les différences
  de versions interdisent de conclure à une compilation identique.
- Les pools matériels sont observés, pas déduits des noms. Le corps de `FX_Atan2`
  est identifié, mais sa table complète n'a pas été comparée. Les signatures
  dégradées de décompilation ne sont pas présentées comme des prototypes fiables.

## Effet sur l'inventaire

Mesure après le second lot, `cargo dq9re coverage` : **Jeu 12 056**, **Plateforme 2 718**,
total **14 780**, **6 exclusions**. Le premier lot attendait 2 682 en Plateforme ; le second
ajoute 36 routines (2 682 + 36 = 2 718), et chaque ligne ajoutée correspond à la taille et
au nom source de la table `VERIFIED_SDK`. Les totaux restent ceux du parent : seules les
routines déjà comptées en Jeu passent en Plateforme.

## Second lot : primitives matérielles identifiées par comportement

Ce lot ajoute **36 routines** (9 CP15, 27 G3X/affichage), soit **1 228 octets** de code.
Chacune est une routine feuille dont le corps n'est qu'une primitive matérielle : aucun appel
vers du code Level-5, aucune structure de jeu. Elles sont classées `Plateforme` dans
`crates/dq9re/src/coverage.rs` (`VERIFIED_SDK`), avec la clé `func_<adresse USA>` : la
comparaison ne se fait pas sur le nom descriptif. Les noms descriptifs sont dans
`re/symbols.txt`, section « behaviour-identified hardware primitives ».

Les noms canoniques du SDK probables sont des **[hypothèse]** : les routines ne sont pas
identifiées par référence à la source, seulement par leur comportement observé.

### Méthode

- Désassemblage Capstone (ARM et Thumb) de la section ARM9 Main en adresses EUR, puis
  balayage de 80 candidats feuilles. Critère : aucune instruction `bl`/`blx`, aucune sortie
  hors de la routine, et un accès I/O (littéral dans `0x04000000..0x04FFFFFF`), `p15`,
  `swi` ou `msr`.
- Chaque candidat retenu a été désassemblé en entier. Les corps finissant par `bx ip` via un
  littéral (appels terminaux) ne sont pas des feuilles au sens de la règle et sont exclus.
- Adresses EUR : USA + `0x8` ou + `0x10` selon la plage (décalages de `AGENTS.md`, vérifiés sur `re/functions.txt`).
- Sémantique : d'après l'encodage des registres. Les bits de GBATEK marqués `[hypothèse]`
  ne sont pas vérifiés point par point dans ce lot ; `[inconnu]` signale un bit non tranché.

### Routines CP15 (9)

| Adresse EUR | Taille | Source | Nom descriptif | Encodage | Sémantique | Feuille |
|---|---|---|---|---|---|---|
| `0x020c89d4` | `0x10` | `func_020c89c4` | `Cp15_EnableIRam` | registre de contrôle, `orr #0x40000` | bit 18 : activation de la I-RAM | oui, pas de `bl`, retour `bx lr` |
| `0x020c89e4` | `0x10` | `func_020c89d4` | `Cp15_EnableDRam` | registre de contrôle, `orr #0x10000` | bit 16 : activation de la D-RAM | oui, pas de `bl`, retour `bx lr` |
| `0x020c89f4` | `0x14` | `func_020c89e4` | `Cp15_GetDtcmBase` | `mrc p15,0,r0,c9,c1,0`, `and #0xfffff000` | base de la région DTCM | oui, pas de `bl`, retour `bx lr` |
| `0x020c8a08` | `0x10` | `func_020c89f8` | `Cp15_EnableProtectionUnit` | registre de contrôle, `orr #1` | bit 0 : activation de l'unité de protection `[hypothèse]` | oui, pas de `bl`, retour `bx lr` |
| `0x020c8a18` | `0x10` | `func_020c8a08` | `Cp15_DisableProtectionUnit` | registre de contrôle, `bic #1` | bit 0 effacé `[hypothèse]` | oui, pas de `bl`, retour `bx lr` |
| `0x020c8a28` | `0x14` | `func_020c8a18` | `Cp15_UpdateDataPermission` | `mrc p15,0,r2,c5,c0,2`, `bic r2,r2,r0`, `orr r2,r2,r1`, `mcr p15,0,r2,c5,c0,2` | lecture-modification-écriture du registre de permissions d'accès données : efface les bits de r0, pose ceux de r1 | oui, pas de `bl`, retour `bx lr` |
| `0x020c8a3c` | `0x8` | `func_020c8a2c` | `Cp15_WriteMpuDataRegion1` | `mcr p15,0,r0,c6,c1,0` | région de protection données 1 | oui, pas de `bl`, retour `bx lr` |
| `0x020c8a44` | `0x8` | `func_020c8a34` | `Cp15_WriteMpuDataRegion2` | `mcr p15,0,r0,c6,c2,0` | région de protection données 2 | oui, pas de `bl`, retour `bx lr` |
| `0x020c9c00` | `0xc` | `func_020c9bf0` | `Cp15_WaitForInterrupt` | `mov r0,#0`, `mcr p15,0,r0,c7,c0,4` | attente d'interruption | oui, pas de `bl`, retour `bx lr` |

### Routines G3X et affichage (27)

| Adresse EUR | Taille | Source | Nom descriptif | Registres | Sémantique | Feuille |
|---|---|---|---|---|---|---|
| `0x02020978` | `0x34` | `func_02020968` | `Disp_WriteBg0CntB` | `0x04001008` : `ldrh`, `and #0x43`, OU de r0..r4 aux décalages 14/7/8/2/13, `strh` | empaquetage de BG0CNT moteur B : taille, couleur, base d'écran, base de caractères, débordement | oui, pas de `bl`, `push`/`pop` |
| `0x020209ac` | `0x34` | `func_0202099c` | `Disp_WriteBg1CntB` | `0x0400100a`, même empaquetage | BG1CNT moteur B | oui, pas de `bl`, `push`/`pop` |
| `0x0208c9dc` | `0x34` | `func_0208c9cc` | `Disp_WriteBg0CntB_Alt` | `0x04001008`, `and #0x43`, `push {r4,lr}`/`pop {r4,pc}` | même registre que `Disp_WriteBg0CntB` ; disposition des arguments non tracée `[inconnu]` | oui, pas de `bl`, `pop {r4,pc}` |
| `0x020938dc` | `0x24` | `func_020938cc` | `Disp_IsPowcnt1Bit15Clear` | lecture `0x04000304` bit 15 | renvoie 1 si le bit est à 0 (inversé) | oui, pas de `bl`, retour `bx lr` |
| `0x020c384c` | `0x28` | `func_020c383c` | `Disp_SetDispstatLyc` | écriture `0x04000004` : LYC bits 8–15, bit 7 depuis r0 bit 8 | ligne de comparaison VCount ; bit 7 `[inconnu]` | oui, pas de `bl`, retour `bx lr` |
| `0x020c3874` | `0x34` | `func_020c3864` | `Disp_SetDispstatVBlankIrq` | `0x04000004` bit 3 (`orr`/`bic #8`) | activation de l'IRQ VBlank `[hypothèse]` | oui, pas de `bl`, retour `bx lr` |
| `0x020c3994` | `0x1c` | `func_020c3984` | `Disp_SetDispcntBMode` | `0x04001000` : `bic #7`, puis `orr r0` | mode BG, bits 0–2 de DISPCNT B | oui, pas de `bl`, retour `bx lr` |
| `0x020c5598` | `0x28` | `func_020c5588` | `G3_SetClearColorDepth` | `0x04000350` (CLEAR_COLOR), `0x04000354` (CLEAR_DEPTH) | couleur et profondeur de fond 3D ; bit 15 = drapeau de brouillard `[hypothèse]` | oui, pas de `bl`, retour `bx lr` |
| `0x020c5660` | `0x30` | `func_020c5650` | `G3_GetGxstatMatrixLevel` | lecture `0x04000600` (GXSTAT) | -1 si bit 14 ; sinon niveau de pile de matrices, bits 8–12 `[hypothèse]` | oui, pas de `bl`, retour `bx lr` |
| `0x020c5690` | `0x30` | `func_020c5680` | `G3_GetGxstatBit13` | lecture `0x04000600` | -1 si bit 14 ; sinon bit 13 `[inconnu]` | oui, pas de `bl`, retour `bx lr` |
| `0x020c56c0` | `0x2c` | `func_020c56b0` | `G3_GetBoxTestResult` | lecture `0x04000600` | -1 si bit 0 ; sinon bit 1 (résultat de test de boîte `[hypothèse]`) | oui, pas de `bl`, retour `bx lr` |
| `0x020cedf0` | `0x1c` | `func_020cede0` | `Disp_IsPowcnt1Bit0Set` | lecture `0x04000304` bit 0 (`tst #1`, `movne #1`) | renvoie 1 si le bit 0 de POWCNT1 est à 1 ; rôle matériel `[inconnu]` | oui, pas de `bl`, retour `bx lr` |
| `0x020daf30` | `0x1c` | `func_020daf20` | `Disp_SetDispcntBBits8To12` | écriture `0x04001000` bits 8–12 | activation des BG du moteur B | oui, pas de `bl`, retour `bx lr` |
| `0x020daf64` | `0x18` | `func_020daf54` | `Disp_GetDispcntBBits8To12` | lecture `0x04001000` bits 8–12 | lecture de la même plage | oui, pas de `bl`, retour `bx lr` |
| `0x020dc018` | `0x18` | `func_020dc008` | `Disp_GetDispcntBBits8To12_Copy` | lecture `0x04001000` bits 8–12 | corps identique à `Disp_GetDispcntBBits8To12` | oui, pas de `bl`, retour `bx lr` |
| `0x020e2b88` | `0x1c` | `func_020e2b78` | `Disp_SetDispcntBBits13To15` | écriture `0x04001000` bits 13–15 | fenêtres WIN0, WIN1, OBJWIN du moteur B | oui, pas de `bl`, retour `bx lr` |
| `0x020e2bbc` | `0x18` | `func_020e2bac` | `Disp_GetDispcntBBits13To15` | lecture `0x04001000` bits 13–15 | lecture de la même plage | oui, pas de `bl`, retour `bx lr` |
| `0x020e29c8` | `0x34` | `func_020e29b8` | `Disp_SetWin1RectB` | H `0x04001042`, V `0x04001046` (moteur B) | rectangle de WIN1, arguments x1,y1,x2,y2 | oui, pas de `bl`, retour `bx lr` |
| `0x020e29fc` | `0x34` | `func_020e29ec` | `Disp_SetWin0RectB` | H `0x04001040`, V `0x04001044` (moteur B) | rectangle de WIN0 | oui, pas de `bl`, retour `bx lr` |
| `0x020e2a30` | `0x34` | `func_020e2a20` | `Disp_SetWin1RectA` | H `0x04000042`, V `0x04000046` (moteur A) | rectangle de WIN1 | oui, pas de `bl`, retour `bx lr` |
| `0x020e2a64` | `0x34` | `func_020e2a54` | `Disp_SetWin0RectA` | H `0x04000040`, V `0x04000044` (moteur A) | rectangle de WIN0 | oui, pas de `bl`, retour `bx lr` |
| `0x020e2a98` | `0x28` | `func_020e2a88` | `Disp_SetWininB1Bits` | WININ B `0x04001048` bits 8–13 | masques de WIN1, drapeau d'effet bit 13 | oui, pas de `bl`, retour `bx lr` |
| `0x020e2ac0` | `0x28` | `func_020e2ab0` | `Disp_SetWininB0Bits` | WININ B `0x04001048` bits 0–5 | masques de WIN0, drapeau d'effet bit 5 | oui, pas de `bl`, retour `bx lr` |
| `0x020e2ae8` | `0x28` | `func_020e2ad8` | `Disp_SetWininA1Bits` | WININ A `0x04000048` bits 8–13 | masques de WIN1 (moteur A) | oui, pas de `bl`, retour `bx lr` |
| `0x020e2b10` | `0x28` | `func_020e2b00` | `Disp_SetWininA0Bits` | WININ A `0x04000048` bits 0–5 | masques de WIN0 (moteur A) | oui, pas de `bl`, retour `bx lr` |
| `0x020e2b38` | `0x28` | `func_020e2b28` | `Disp_SetWinoutBBits` | WINOUT B `0x0400104a` bits 0–5 | masques hors fenêtre (moteur B) | oui, pas de `bl`, retour `bx lr` |
| `0x020e2b60` | `0x28` | `func_020e2b50` | `Disp_SetWinoutABits` | WINOUT A `0x0400004a` bits 0–5 | masques hors fenêtre (moteur A) | oui, pas de `bl`, retour `bx lr` |

### Candidats restants (non classés)

- `0x020c9bfc` : contrôle de banque WRAMCNT (`0x04000247`), hors G3X et affichage.
- `0x020292ec` (0x98) : écritures de géométrie G3 `0x04000444..0x040004C0`, arguments
  multiples en virgule fixe ; non décidé.
- `0x020c54a4` : deux chemins de registres plus une branche sur `r0` ; composite.
- `0x020db9a8` : lit le champ `[r0,#0x54]` d'une structure de jeu.
- `0x02074b04`, `0x02074b64` : sauvegarde de l'état BG dans une structure.
- `0x02074c18`, `0x02074e54` : vidage d'une structure de 0x23c octets.
- Appels terminaux via littéral (`bx ip`) : `0x020c516c` (FIFO son), `0x020c555c`
  (`0x04000330`), `0x020c5574` (`0x04000360`), `0x020dbfe0` (`0x0400106c`), `0x020dbff4`
  (`0x0400006c`). Ce sont des enveloppes de primitives, pas des feuilles au sens de la règle.
- Écritures son `0x04000400..0x040004FF` : hors G3X et affichage.
- `0x020cd5a4`, `0x020cd5e4` (DIVCNT), `0x020d1478`, `0x020d14c0` (EXMEMCNT), `0x020d6cac`
  (IME), `0x020d86e0` (`0x04000540`) : registres matériels hors périmètre de ce lot.
- Routines `swi`/`msr` : `0x0200bbe0`, `0x0200bc78`, `0x0200bd10`, `0x0200bdb4`,
  `0x0200bedc`, `0x0200d894`, `0x0200d8a8`, `0x0200d8bc`, `0x020c0338`, `0x020c08ac`,
  `0x020c8b54`, `0x02000970`, `0x02000ab0` : à classer après lecture du contexte d'appel.

Ces candidats restent non classés. Le compte de Plateforme n'augmente que par les routines
listées ci-dessus.
