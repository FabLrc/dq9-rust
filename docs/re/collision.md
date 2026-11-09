# Map collision (`.col2`)

The reader `dq9_formats::map::Collision::parse` decodes the **decompressed** bytes of the mesh,
the grid index and the floor records of European `.col2` files. In the archives, `.col2`
members carry a Nintendo compression header; the caller decompresses them with
`nds_rom::compress::decompress` before parsing. The parser does no I/O.
The parser is consumed by `dq9-core` for floor queries and movement
([`movement.md`](movement.md)). Construction stays I/O-free; archives and members are
supplied by the caller.

## Observed structure

The format has no text signature. The first word is 3 in all 1,178 files of the ROM. The
offsets of the header table point to the sections; the reader checks their bounds and
consistency before building the result.

| Offset | Data | Decoding |
|---|---|---|
| `0x00` | version | `u32`, value 3 |
| `0x04` | `shift` | `u32`, 0 to 5; vertices and bounds are `integer << shift` in `fx32` |
| `0x08` | bounds | six `i16`, min x/y/z then max x/y/z |
| `0x14` | triangle count | `u32` |
| `0x18` | cell dimensions | `u16` size, `u16` unknown |
| `0x1c`, `0x20` | grid dimensions | `u32` X and Z |
| `0x24`…`0x30` | offsets | triangles, counts, starts and indices per cell |
| `0x34`, `0x38` | floor records | count `u32`, offset `u32` |

Each triangle is 28 bytes: three vertices `i16[3]`, a signed 1.3.12 normal `fx16[3]`, then a
`u32` attribute word. Coordinates are integers in the units of the bounds, not fx32 values.
Trailing records are 8 bytes; only their first two `u16` have an established
interpretation. On ordinary maps, the first decodes terrain digits for the battle stage
identifier; on the sky map, bits 5–9 of the second are used for landing permission.

The grid has `gridX * gridZ + floor(gridZ / 2)` cells. For each one the file stores a `u8`
count and a `u16` start into a `u16` index list. The reader requires consecutive ranges,
valid triangle indices and at most one trailing zero `u16` for alignment. It keeps the ranges
as contiguous indices (`cells` and `cell_indices`), with no per-cell allocation.

## Evidence and limits

The format facts come from the [Map-Collision page of the DQIX wiki](https://github.com/DQIX/wiki/blob/main/Map-Collision.md),
re-observed on the European ROM `YDQP01`: it reports exhaustive checks over 1,178 files,
109,122 triangles, normals matching the cross product for every non-degenerate triangle and
grids covering the bounds. The game's collision code does not yet validate the reader here.

The test `collision_meshes_parse` opens every `.amdj` archive of the ROM, decompresses its
`.col2` member, then checks the 1,178 meshes (109,122 triangles) through `Collision::parse`.
Synthetic tests check decoding, truncated sections and inconsistent ranges. The ROM is not a
fixture and is not needed by CI.

Still unknown: the exact meaning of the last two half-words of the records; the `2^shift`
factor is confirmed by the code (`0x0204bf54`: bounds `<< shift`; `ov_017::02192b48`: vertices × `1 << shift`).
Some small meshes are likely vertical
volumes, not walkable surfaces. The reader exposes this data without assigning it any
physics.

## Use by J2

`dq9_core::CollisionWorld` keeps the `Collision` meshes as read and their placements
(`Piece`: mesh, position and `fx32` angle of the placement); the game works in the frame of
each piece, vertices = file integers `<< shift`. Movement, the floor ray and wall push-out
are described in [`movement.md`](movement.md).

## Grid, attribute word and query (J2, `0x0204c040`)

The query code establishes (re-read in `dq9_core::collision::query` and checked by synthetic
tests):

- **In-memory `.col2` object** (`0x0204bf54`, 0x4c bytes): `+0` header, `+4` triangles,
  `+8` cell counts (`u8`), `+0xc` starts (`u16`), `+0x10` indices (`u16`), `+0x14`
  records (version 3), `+0x18` max bounds `<< shift`, `+0x24` min bounds `<< shift`,
  `+0x48` cell count `gridX × gridZ + gridZ / 2`.
- **Bounds test**: the query box (in piece coordinates) must overlap the mesh bounds (edges
  inclusive), then it is shifted by `shift` (arithmetic shift); a triangle is kept if its
  box overlaps it.
- **Triangle box**: the attribute word (`+0x18`) is made of bytes `0x18..0x1a`: six index
  nibbles (into the triangle's 9 coordinates) for min x/y/z then max x/y/z; byte `0x1b` is
  `(record index << 1) | temporary mark`. This explains the "permutations of `0x543210`"
  values in the wiki.
- **Staggered grid**: with at most 7 cells, all are scanned; otherwise row =
  `⌊FX_Div(zc − minZ, size) / 4096⌋` (truncated toward 0), column = same on `x` (with
  `+ size / 2` on odd rows), `zc` and `xc` being the box centers; the central cell has
  index `row × gridX + column + row / 2` and its neighbors (six around an even row, seven
  depending on the position on an odd row, which has `gridX + 1` cells) are read too. A value
  slightly outside the grid (less than one cell short) falls, through truncation, into the
  first column or row.
- At most 0x60 triangles per piece and per query, 0xc0 candidates examined.

The reader `Collision::parse` already exposes these words (`Triangle::attributes`).
