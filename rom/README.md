# rom/

Place your own dump of the game here (any `*.nds` name). Everything in this directory except
this file is git-ignored: the ROM and any data extracted from it are copyrighted and must never be
committed or distributed.

Expected image: `Dragon Quest IX - Sentinels of the Starry Skies (Europe)`, game code `YDQP01`,
268 435 456 bytes, header CRC `0xF9CC` (`cargo dq9re info` checks it).

Tools also find a ROM in the repository root, or via `--rom <path>` / `DQ9_ROM`.
