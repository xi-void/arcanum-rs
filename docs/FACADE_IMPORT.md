# Baked facade pieces in Bates Mansion

```sh
game="$HOME/Work/arcanum-ce-test"
cargo run --locked --features windowed -- art sector-facade "$game"
```

The 9 × 9 preview uses local tiles (35, 14) through (43, 22) of
`maps/bates mansion lev 1/67108865.sec` from `modules/Arcanum.dat`. The
world center is (103, 82). The 15 rug pieces and 66 surrounding floor
cells all come from the unmodified sector words. The other two sector
previews and the floor-only audit retain their original scope and
commands. Optional `--mods` arguments use the existing VFS precedence for
the sector, both name tables and artwork.

## Original data

| Facade number | Sector cells | Distinct words | Winning ART in `Arcanum2.dat` |
|---:|---:|---:|---|
| 223 | 15, one 5 × 3 rug | 15 | `art/facade/rug3-flip.art` |
| 225 | 90, six 5 × 3 rugs | 15 | `art/facade/rug4-flip.art` |

The first rug occupies local x 37–41 and y 17–19. Reading the little-endian
sector words after the nine light records gave the following frame
indices in cell traversal order.

```text
 0  1  2  3  4
 9  8  7  6  5
10 11 12 13 14
```

For example, `0xb5be0801` at (37, 17) selects frame 0, and `0xb5be0813`
at (37, 18) selects frame 9. Across both rugs every word stores terrain
number 1, indoor type, flippable capability and walkable bit set.
The public `FacadeId` keeps the original word. Its accessors derive
the following fields, and each loaded draw retains the resource
provenance through the shared ART loader. [ID decoder](../src/facade.rs)
· [Sector surface importer](../src/content/sector.rs)

| Bits | Meaning |
|---|---|
| 28–31 | Facade category, `0xb` |
| 27, 17–24 | High bit and low byte of the facade number |
| 26 | Flippable capability |
| 25 | Outdoor type |
| 11–16 | Terrain number |
| 1–10 | Zero-based piece/frame index |
| 0 | Walkability metadata |

CE revision `fb721982` establishes the fields in TIG's
`tig_art_facade_id_create` and the associated getters. CE's
`name_normalize_aid` reduces the ID for filename lookup. `a_name.c`
indexes the names in numeric MES order starting at key zero. The
winning `art/facade/facadename.mes` in `Arcanum2.dat` has 373 contiguous
entries. Here ordinal and key coincide. Rust preserves the ordinal
rule for tables with gaps and validates the resulting filename as a
single safe stem. The frame comes from the original ID. TIG selects
direction zero and palette zero for this category.

Both installed flip ARTs store header flags `0x09`, 8-bit indices, one
palette in slot zero, 15 RLE frames, 8 stored FPS and action marker 14.
All pieces are 78 × 40 with hotspot (39, 39), offset (0, 0) and 1,600
opaque pixels in the same diamond mask as the floor tiles. The header
ends at byte 132, the palette at 1,156, and the table at 1,576.
The pixel payloads end at the file lengths of 28,135 and 27,980 bytes,
respectively. The decoder now accepts the exact flag combination
`0x09` with the existing bounds, pixel encoding and palette checks.
TIG's `sub_51BE30` selects one stored direction using bit 0. The
purpose of bit 3 is unverified. [ART format](ART_FORMAT.md)

## Placement, order and orientation

CE's `tile_draw_iso` reads sector words directly. It submits the selected
facade frame in the same pass as the adjacent floor. It clips each
78 × 40 source to the tile rectangle starting at
`location_xy + (1, 0)`, with source coordinates relative to that
rectangle. Rows advance in y, with x advancing within each row. In the
native viewport, relative tile coordinates (x, y) give
`center = (40(y − x), 20(x + y))`. The sprite raster starts at
`center − (39, 20)`. The compositor hotspot anchor therefore lands at
`center + (0, 19)` for this verified geometry. The preview submits the
cells in row order and applies no additional facade overlay.
[Projection](ISOMETRIC_FLOOR.md)

TIG's `tig_art_blit` applies automatic tile mirroring only to the tile
category. `tig_art_id_flags_get` returns zero for facades. The
`-Flip` names refer to distinct installed assets paired with their own
piece layouts. For example `Rug3` has a 3 × 5 layout and
`Rug3-Flip` has a 5 × 3 layout. Independently composing both from the
original walkmasks gave mirrored silhouettes and different RGB
samples. The sector renderer selects the baked art and positioning as
stored. It preserves both capability and walkability metadata and
applies no second pixel mirror.

CE's editor runs `facade_draw` separately after `tile_draw`. Its
`sub_4CA0F0` builds the temporary preview from a standalone
`FacWalk V101` file. The inspected `art/facade/facwalk.df` in
`Arcanum2.dat` is 218 bytes. The header gives terrain 1, outdoor 0,
flippable 1, size 5 × 3 and 15 entries. The entries supply local x,
local y and walkability, with frame implicit in record order.
Those positions and flags match the rug already baked into the Bates
sector. The normal game draw calls `tile_draw` before `object_draw`
with no separate `facade_draw` call. This sector importer reads the
baked cells without a walkmask dependency.

Unsupported paths include editor walkmask placement, other ART
encodings and facade pieces outside the verified geometry. Wrong
category, missing names, missing resources, invalid frame indices and
malformed ART produce explicit diagnostics. The importer does not
infer a terrain fallback or reuse a wall ID as a facade.

## Conventional walls

Conventional wall objects are loaded through the sector's separate
`objlist_load` path after the roofs and sector metadata. CE draws them
in `object_draw`, which uses object location, pixel offsets, flags and
render ordering. Wall variants have their own piece, damage and naming
rules in `a_name.c`, including `art/wall/wallname.mes`. The next bounded
step is to frame the intervening sector sections, decode one verified
wall object record, and validate the winning wall artwork, pivot and
draw order relative to the floor. The facade preview leaves object drawing and roof visibility outside
its scope.

## Verification and measured costs

The initial Wayland check established presentation, resize and idle
behavior. The rug's visual appearance was challenged after that check.
The follow-up compared the composed pixels with a separate decoder
and the visible portion of a published gameplay screenshot. See
[the fidelity check and piece trace](#fidelity-check-and-piece-trace).

An optimized Rust 1.100.0 probe on Linux 7.2.15 and an Intel i5-6300U
used one pinned logical CPU, one warm-up and seven measured batches.
Decode-only timing uses previously read bytes and includes the native
RGBA allocations. The VFS timings reuse mounted archives and include
sector and MES reads, ART inflation and decoding. The importer reads
each selected ART path once and retains only the requested frames.
Rendering uses 81 draws and preallocated 800 × 600 buffers. These
measurements exclude window presentation.

| Stage | Repetitions per batch | Median µs [min, max] |
|---|---:|---:|
| Decode all 15 Rug3-Flip frames | 500 | 48.45 [46.08, 50.21] |
| Import 9 × 9 floor through VFS | 60 | 961.79 [945.37, 1001.54] |
| Import 9 × 9 facade region through VFS | 60 | 1211.23 [1197.90, 1355.12] |
| Compose 9 × 9 floor | 250 | 476.79 [471.93, 481.53] |
| Compose 9 × 9 facade region | 250 | 476.50 [467.66, 486.55] |
| Clear, compose and pack 9 × 9 floor | 140 | 1962.35 [1948.97, 1992.49] |
| Clear, compose and pack 9 × 9 facade region | 140 | 1987.24 [1939.44, 2010.26] |

The facade scene retains 23 distinct decoded frames across nine ART
paths, or 287,040 pixel bytes. It needs no additional mirror frames.
Its RGB8 and packed buffers together account for about 3.20 MiB,
excluding the presentation backend. The render timings overlap those
of the floor-only scene.

Formatting, Clippy, default and windowed tests, debug and release
builds, and `git diff --check` passed. All 130 portable tests passed in
an asset-free network namespace. All 18 installed-data checks passed,
including `vfs-smoke`. Both schematic audits returned 119/119. All
five headless demos ran in isolation.

## Fidelity check and piece trace

The sector contains one draw ID per cell. The native importer takes
either the original floor frame or the original facade frame from that
word. There is no wood draw under a facade cell. Some facade frames
themselves contain plank pixels around the edge of the carpet pattern.

Independently decoding the two 218-byte walkmasks, `facwalk.df` and
`facwalk.e1`, gives the same sequence of 15 records. In
`walkmask_read_data` the **zero-based record number is the ART frame**
and the record's x and y select the grid cell. The editor's
`facade_draw` reads that grid by row. The normal game's `tile_draw_iso`
already has the baked frame IDs at those cells. Its 78 × 40 blit starts
at `location_xy + (1, 0)`. The editor requests a separate 80 × 40 preview
rectangle and uses the same starting offset for the isometric source.
The sector renderer follows the normal game tile pass.

The table lists record/frame order for both rugs. The Rug3-Flip origin
is sector-local (37, 17), world (101, 81). The compared Rug4-Flip
instance uses sector-local (37, 38), world (101, 102). The other five
Rug4-Flip instances start at sector-local (22, 29), (27, 29), (32, 29),
(37, 29) and (42, 29). All 105 sector cells agree with their respective
walkmask records and metadata. Screen positions are frame top-lefts
and hotspot anchors in the 800 × 600 view. The compared 9 × 9 patches
are centered identically relative to each rug, so the screen
positions apply to both.

| Walkmask record / ART frame | Walkmask x, y | Rug3 world x, y | Rug4 world x, y | Raster x, y | Hotspot anchor x, y |
|---:|---|---|---|---|---|
| 0 | (0, 0) | (101, 81) | (101, 102) | (401, 220) | (440, 259) |
| 1 | (1, 0) | (102, 81) | (102, 102) | (361, 240) | (400, 279) |
| 2 | (2, 0) | (103, 81) | (103, 102) | (321, 260) | (360, 299) |
| 3 | (3, 0) | (104, 81) | (104, 102) | (281, 280) | (320, 319) |
| 4 | (4, 0) | (105, 81) | (105, 102) | (241, 300) | (280, 339) |
| 5 | (4, 1) | (105, 82) | (105, 103) | (281, 320) | (320, 359) |
| 6 | (3, 1) | (104, 82) | (104, 103) | (321, 300) | (360, 339) |
| 7 | (2, 1) | (103, 82) | (103, 103) | (361, 280) | (400, 319) |
| 8 | (1, 1) | (102, 82) | (102, 103) | (401, 260) | (440, 299) |
| 9 | (0, 1) | (101, 82) | (101, 103) | (441, 240) | (480, 279) |
| 10 | (0, 2) | (101, 83) | (101, 104) | (481, 260) | (520, 299) |
| 11 | (1, 2) | (102, 83) | (102, 104) | (441, 280) | (480, 319) |
| 12 | (2, 2) | (103, 83) | (103, 104) | (401, 300) | (440, 339) |
| 13 | (3, 2) | (104, 83) | (104, 104) | (361, 320) | (400, 359) |
| 14 | (4, 2) | (105, 83) | (105, 104) | (321, 340) | (360, 379) |

The walkmask record order places frame 5 at the right end of row 1 and
frame 9 at its left end. Draw traversal visits frame 9 first on that
row. Both assets use hotspot (39, 39), zero movement offsets, palette
slot zero and 1,600 nontransparent pixels per frame. The stored
flippable and walkable bits do not trigger mirroring in TIG's facade
blit. The selected `-Flip` files supply the visible orientation.

A separate Python reader decoded the little-endian sector and walkmask
records, original palettes and RLE packets. It placed each rug by
its walkmask coordinates, filled the other cells from their sector
words and verified that no two opaque pixels shared a screen coordinate.
The complete unlit RGB8 framebuffers matched the native compositor.
The installed regression pins their independent FNV-1a fingerprints
`6e6eed198e622b3b` for Rug3-Flip and `17861112b6976e51` for Rug4-Flip and
checks each selected record and its ART metadata. The portable
regression checks that a transparent facade edge leaves the background
visible, even when the neighboring floor frames are opaque.

The native Wayland captures under `target/visual-checks/milestone-32/`
are direct `grim` captures of the drawable region, close-up region and
resized drawable region. Their dimensions are 800 × 600, 380 × 200
and 180 × 135. At 1.25× display scaling, aligning the window origin to
physical pixels eliminated compositor sampling from the capture.
Each PNG then matched the relevant region of the independent unlit
reference pixel for pixel. The resized capture uses the same logical
scene center.

The visible motif and orientation also match the Rug3-Flip in the
published Gilbert Bates dialogue screenshot. A visible interior
sample produced a normalized detail correlation of 0.97 at the
published image's scale, compared with 0.20 when the sample was mirrored.
That screenshot includes a heading, dialogue and actors over parts of
the rug. The compared area is darker in that gameplay image than in the
native unlit view. [Published gameplay screenshot and context](https://lilura1.blogspot.com/2020/06/Arcanum-Walkthrough-Guide-Part-X-Tarant-Gilbert-Bates-Estate-Persuasion-Infiltration-Disguise-Stennar-Rock-Cutter.html)

CE's `light.c::sub_4DE0B0` selects indoor or outdoor ambient tint for
facade palettes. Its `tile_draw_iso` also samples `sub_4DA360` and
can apply a constant tint or interpolate color in four quadrants.
The native preview displays the stored palette at full intensity.
The gameplay screenshot supports the Rug3-Flip layout and orientation.
Exact in-game color, lighting and the complete occluded scene remain
unverified. Rug4-Flip was verified through the independent unlit
composition. The checks identified no importer discrepancy that
warrants changing production placement or rendering.
