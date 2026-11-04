# NitroSDK ARM9 : premier lot vérifié (50 routines)

Ces **50 routines de `Module::Main` européen** sont classées **Plateforme**, non
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

Le lot représente **10 168 octets**, pools inclus. Avant régénération, les
50 entrées étaient Jeu : déplacement attendu **12 142 → 12 092 Jeu** et
**2 632 → 2 682 Plateforme**, total **14 780** et **6 exclusions** inchangés.
Ces valeurs attendues doivent être confirmées par la régénération CLI du parent ;
ce ne sont ni 50 ports Rust ni un nouveau résultat de couverture publié ici.
L'identification globale du SDK reste ouverte pour toutes les autres routines.
