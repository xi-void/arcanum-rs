# World camera

Launch the default Bates Mansion Level 1 scene with:

```sh
cargo run --locked --features windowed -- art world "$HOME/Work/arcanum-ce-test"
```

Arrow keys pan by 16 framebuffer pixels. Hold Shift to pan by 64 pixels.
Home restores the initial camera target and Escape closes the window. The
default target is world tile (101, 83). The existing `--view LOCAL_X LOCAL_Y SIDE
CENTER_X CENTER_Y WIDTH HEIGHT` option selects a smaller loaded region, a
different target, or a different initial viewport. `art world-inspect`
continues to inspect the fixed overview by default. Its `--view` can inspect
another region, including the full sector.

For the original-map streaming mode, see [Multi-sector world navigation](MULTI_SECTOR_WORLD.md).

## Geometry and cache

The default viewer loads all 4,096 surface cells of
`maps/bates mansion lev 1/67108865.sec` once. The installed sector selected
through the VFS has 3,991 floor entries and 105 facades, with 268 supported walls,
65 scenery objects and 19 portals. Startup reports 16 unsupported scenery records with their original
record indices and causes. All original wall records resolve.
The preview uses base-sector data.

The existing projection maps reference-relative world tile deltas to
`(40 × (dy − dx), 20 × (dx + dy))`. The camera subtracts the same pixel offset
from that projection for every layer. It constrains the viewport in the two
diagonal axes of the loaded tile footprint. When the viewport is too large to
fit, the camera centers over the region and the compositor clears exposed
pixels to its normal background. Resizing reevaluates the original Home target
when the camera is at Home.

The scene prepares ordered raster rectangles once from the existing surface
and static-object draw lists. Each redraw selects intersecting raster
rectangles, including overhang from sprites whose tile anchors are outside
the viewport. The draw order, pivots and clipping remain the existing
compositor's responsibility. A stationary camera doesn't schedule redraws.

The loaded scene retains 169 distinct decoded or transformed rasters with
3,623,628 bytes of RGBA pixel storage. The other 4,279 draw references reuse
those rasters. Navigation makes no VFS accesses or ART decode calls and
allocates no draw list per frame. Selection currently scans the prepared
rectangles linearly within the single loaded sector.

The available objects are limited to the existing supported forms. Adjacent
sectors, save overrides, lighting and gameplay visibility remain outside this
preview.


## Kitchen corner verification

The two missing arms were both original wall records at tile (78, 78).
The resolver previously rejected legacy wall pieces 5 and 6. CE's
`build_wall_file_name` identifies them as `tcl` and `tcr` respectively.
The original IDs, frame zero, offsets and established wall pivot and draw
order supply the entire corner.

| Record and sector bytes | Tile | ART ID | Rotation, palette, mirror | ART basename and archive | Pivot | Raster origin relative to (101, 83) |
| --- | --- | --- | --- | --- | --- | --- |
| 0, 18428–18511 | (78, 78) | `0x10318800` | 1, 0, no | `in4tcru0.art`, Arcanum5.dat | (−1, 151) | (721, −711) |
| 1, 18511–18594 | (78, 78) | `0x10317811` | 7, 1, yes | `in4tclu0.art`, Arcanum2.dat | (39, 151) | (681, −711) |
| 3, 18689–18772 | (79, 78) | `0x10303811` | 7, 1, yes | `in4bseu0.art`, Arcanum2.dat | (39, 152) | (641, −692) |
| 49, 22759–22842 | (78, 79) | `0x10300800` | 1, 0, no | `in4bseu0.art`, Arcanum2.dat | (−1, 152) | (761, −692) |

All four inherit (0, 0) pixel offsets and 100% scale from serialized
prototype 1021. The two new rasters have stored hotspot (39, 131) and
zero frame offsets. Their rotation and mirroring use the existing wall
rules. At the kitchen-edge camera offset (+720, −460), the correction
changed 8,143 pixels, confined to x=601–680 and y=108–256.

Panning 64 pixels down from the kitchen edge gives (+720, −396).
The close-up pixels match exactly after applying that translation.
The software framebuffer comparison is exact across the full shared
rectangle in the installed-data regression. In the two Wayland captures,
the bottom compositor-edge row differs, with all other 838,400 shared
pixels identical. Initial and Home-restored 1280 × 720 captures are also
identical across all 921,600 pixels.

The supplied CE kitchen capture was registered at 1.8 CE screen pixels per
native pixel using the twin stovepipes, barrel, pots and stove range.
The corner rail matches at the same registration within one native pixel
of sampling. The CE capture still differs in lighting, modulation, shadows,
and the presence of actors and interface artwork.


## Exterior walls and sector-wide audit

All 268 original wall records, representing 73 distinct ART words, are
parsed and resolved. The original resolver accepted 232. Its 36 rejections
were the two kitchen arms and 34 exterior pieces. The latter are records
329, 331–337, 339–350, 352–353, 355–360 and 362–367. The run lies at Y=110
with rotation 3, palette 1 and horizontal mirroring. Rotation 3 selects the
exterior stem, like rotation 5. CE chooses the exterior family for
rotations 2–5 in `a_name_wall_aid_to_fname`. Both facings use the existing
directional pivot and object-order rules. The two missing lower-corner
suffixes are `bcl` and `bcr` in CE's filename table.

| Record | Tile | ART ID | Original frame-zero resource | Rotation, palette, mirror | Effective pivot |
| --- | --- | --- | --- | --- | --- |
| 329 | (78, 110) | `0x10305911` | `gb1lfcu1.art` | 3, 1, yes | (−1, 131) |
| 347 | (94, 110) | `0x10301911` | `gb1bseu1.art` | 3, 1, yes | (−1, 131) |
| 365 | (109, 110) | `0x10301811` | `gb1bseu0.art` | 3, 1, yes | (−1, 131) |
| 366 | (110, 110) | `0x1030e900` | `gb1bclu1.art` | 5, 0, no | (39, 131) |
| 367 | (110, 110) | `0x10311911` | `gb1bcru1.art` | 3, 1, yes | (−1, 132) |
| 327 | (110, 109) | `0x10302900` | `gb1bseu1.art` | 5, 0, no | (39, 131) |

These resources come from Arcanum2.dat. Positions use the original tiles
and inherited (0, 0) offsets. Record 327 was already visible and joins
the restored corner. The eastern and southern views show different parts
of the same restored perimeter. At a 64-pixel camera displacement in each
view, every shared pixel in both 1280 × 720 native captures matched.
The available CE gameplay references include the kitchen but do not show
the matching exterior face. The exterior is verified against the sector,
resource bytes and CE's naming and placement rules.

| 1280 × 720 camera offset from tile (101, 83) | Wall draws submitted | Culled by raster bounds | Rejected |
| --- | ---: | ---: | ---: |
| Sitting room (0, 0) | 96 | 172 | 0 |
| Dining and staircase (+512, −64) | 130 | 138 | 0 |
| Entrance (+80, +352) | 79 | 189 | 0 |
| Kitchen (+720, −460) | 87 | 181 | 0 |
| Kitchen inset (+720, −396) | 97 | 171 | 0 |
| Southern run (+1360, +400) | 71 | 197 | 0 |
| Southern run shifted left (+1296, +400) | 72 | 196 | 0 |
| Eastern corner (+840, +620) | 57 | 211 | 0 |
| Eastern corner shifted right (+904, +620) | 56 | 212 | 0 |
| Far exterior (+1400, −100) | 114 | 154 | 0 |

Every wall is submitted in at least one sampled viewport. Submitted draws
can be partially clipped or subsequently occluded by another original
sprite. The generated audit enumerates every record's location, original
ART word, resolved VFS path, pivot, palette, bounds, prior rejection and
selection status in each sampled view.

## Fresh-process Wayland replay

The 1280 × 720 native viewer visited the sitting room (0, 0), kitchen
(+720, −460), eastern exterior (+832, +624), southern exterior
(+1360, +400), and staircase (+512, −64) twice in a single fresh
process. Coordinates are framebuffer pixels relative to tile (101, 83).
All five repeated captures and the final Home capture matched pixel for
pixel. The kitchen and exterior views were each resized to 960 × 560
between passes. Their restored 1280 × 720 captures matched exactly. The
smaller screenshots matched the centered crop above the last
presentation-edge row. The installed-data regression independently
compares the entire software framebuffer crop, including that row.

Wayland idle sampling detected zero buffer attachments during six
three-second stationary intervals. Gameplay state is the original static
base-sector state throughout navigation.

## Release performance

Linux, Rust 1.100.0 and Intel Core i5-6300U pinned to CPU 1.
The benchmark used 1280 × 720 RGB and packed framebuffers and excluded
Wayland presentation. For stationary timing, one warm-up pair preceded
seven alternating batches of 100 frames per variant. Selection timing
used seven batches of 480 camera steps. Navigation timed 240 individual
frames with 663–809 visible draws.

| Measurement | Median | Range |
| --- | ---: | ---: |
| Pan and raster selection | 0.013 ms | 0.0119–0.0179 ms |
| Fixed overview, same-run control | 6.990 ms | 6.877–7.074 ms |
| Stationary navigable scene, clear through pack | 7.420 ms | 7.266–7.858 ms |
| Navigating scene, clear through pack | 6.460 ms | 5.075–9.209 ms |

The historical fixed-overview median was 6.901 ms. The same-run paired
median increase was 0.456 ms. The navigation p95 was 8.676 ms. A separate
unpaired run had a 142.8 ms outlier of undetermined cause. Sector import
and scene preparation took 50.4 ms after the VFS had been mounted in the
paired run. All subsequent draws in both runs used retained frames and
there were no VFS or decoding misses during navigation.
