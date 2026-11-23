# Connected Bates Mansion walls

```sh
game="$HOME/Work/arcanum-ce-test"
cargo run --locked -- art sector-walls-inspect "$game"
cargo run --locked --features windowed -- art sector-walls "$game"
```

The scene loads 17 wall objects from
`maps/bates mansion lev 1/67108865.sec` in `modules/Arcanum.dat`.
The 9 × 9 surface patch spans world X 97–105 and Y 77–85. Its center
(101, 81) is shared by the original floor, transitions, facade and
walls. Record 107 at (98, 86) lies outside the surface patch and still
contributes its visible pixels. The existing `sector-wall` preview
keeps its original surface region and camera center.
[Single-wall format evidence](WALL_OBJECT_IMPORT.md)

## Original wall inventory

All 17 records are type 0 and use prototype 1021. The selected instance
and prototype properties give offset (0, 0) and scale 100% in this
installation. All artwork names in the table are relative to
`art/wall/` and come from `Arcanum2.dat`. The diagnostic also
reports each complete path, palette, mirror flag and VFS source.

| Record | World tile | Stored ART ID | Rotation | Artwork |
|---:|---|---|---:|---|
| 30 | (97, 78) | `0x10303811` | 7 | `in4bseu0.art` |
| 31 | (98, 78) | `0x10300800` | 1 | `in4bseu0.art` |
| 32 | (98, 78) | `0x10303811` | 7 | `in4bseu0.art` |
| 33 | (99, 78) | `0x10303811` | 7 | `in4bseu0.art` |
| 34 | (100, 78) | `0x10397911` | 7 | `in4p4lu1.art` |
| 36 | (101, 78) | `0x1039b911` | 7 | `in4p4au1.art` |
| 37 | (102, 78) | `0x1039f911` | 7 | `in4p4bu1.art` |
| 38 | (103, 78) | `0x103a3911` | 7 | `in4p4ru1.art` |
| 39 | (104, 78) | `0x10303811` | 7 | `in4bseu0.art` |
| 52 | (98, 79) | `0x10300900` | 1 | `in4bseu1.art` |
| 60 | (98, 80) | `0x10370800` | 1 | `in4d6lu0.art` |
| 69 | (98, 81) | `0x10374800` | 1 | `in4d6au0.art` |
| 77 | (98, 82) | `0x10378800` | 1 | `in4d6bu0.art` |
| 87 | (98, 83) | `0x1037c800` | 1 | `in4d6cu0.art` |
| 93 | (98, 84) | `0x10380800` | 1 | `in4d6du0.art` |
| 99 | (98, 85) | `0x10384800` | 1 | `in4d6ru0.art` |
| 107 | (98, 86) | `0x10300800` | 1 | `in4bseu0.art` |

Records 31 and 32 form the corner at (98, 78). The rotation 7 arm
includes the four bookshelf pieces. The perpendicular arm includes
the six-piece door opening. The short wall visible behind the corner
is the continuation from (97, 78). The opening includes its original
transparent pixels. Portals and other objects are outside this pass.

## Artwork, palette and pivot

CE's `a_name_wall_aid_to_fname` chooses interior art for rotations
1 and 7 via `structure.mes` and `wallname.mes`. The undamaged piece
suffix is `bse`, one of `d6l/a/b/c/d/r` or one of `p4l/a/b/r`.
Each selected ART stores one direction, one frame, and both palette
slots. Rotations address frame zero. Rotation 1 uses the pixels as
stored with palette zero. Rotation 7 uses palette one with mirrored
pixel rows. The wall cache path in TIG maps the odd rotation to its
even partner within the same cardinal direction and preserves the
selected orientation and palette. Single-direction storage aliases
those direction slots. All selected ARTs contain their requested
palette. The decoder implements CE's slot-zero fallback when slot
one is absent, and the default ART inspection still selects slot
zero. [ART decoder](../src/art.rs) · [Wall ART resolution](../src/wall.rs)

The original frames have a 40-pixel width and hotspot (39, 132).
Heights vary across the base, bookshelf and opening pieces. The
original `tig_art_frame_data` first adjusts the selected interior
hotspots by (−40, +20) and then adjusts mirrored X to
`width − adjusted_x − 2`. The effective pivot is therefore
(−1, 152) for rotation 1 or (39, 152) for rotation 7. The renderer
preserves the stored hotspot. It applies the pivot through anchor
placement and mirrors only the pixels. Instance offsets are added
to the projected tile center. Scaled, damaged or translucent walls,
other facings and unverified piece families report unsupported
appearance or ART fields. The importer bounds the selection and
retained frame sizes and reuses decoded frames by artwork path,
palette and mirror state. [Wall loader](../src/content/wall.rs)

## Ordering and overlap

CE draws the surface pass before the upright object pass.
`object_draw` traverses sector tiles by increasing y and then x.
`sub_4F20A0` inserts upright walls into each tile's object list
using the depth and horizontal keys computed by `sub_4F2230`.
For walls, rotation picks the nominal order offset (0, −20) or
(0, 19), independent of actual pixel offsets. `sub_4B93F0`
maps those offsets to (depth, horizontal) keys (−40, 0) or
(−2, 38). Equal wall keys use original insertion order. The
selected 1 and 7 facings both use (−40, 0). The native batch
preserves that ordering, including the equal-key pair at the
corner, even when callers supply the selection in reverse.
Mixed object categories and dynamic ordering remain outside the
wall-only pass.

The independently decoded opaque projections of records 30 and
31 overlap at 4,568 pixels. At drawable pixel (401, 28), record 31
overdraws RGB (92, 88, 90) with (47, 48, 49). The installed
regression checks this pixel and pins the full RGB8 FNV-1a
fingerprint `c65c248fbb57a534`. The portable regression checks
that a transparent wall pixel preserves the earlier draw.
The camera-center regression shifts all 17 walls and all 56
common surface cells by the same (+40, −60) when the view
is reframed around (103, 82).

## Visual verification

The full 800 × 600 Wayland view, the 400 × 320 corner close-up
and the resized 400 × 400 view are real `grim` captures. Each
matched the corresponding pixels of an independent unlit
composition from the original sector and ART bytes. Resizing
clipped the frame around the same map center, submitted one
fresh buffer and remained idle for 750 ms without another
buffer submission.

A published gameplay image shows the same rug and room, with a
different crop, gameplay objects, lighting and dialogue
overlays. It does not expose the entire selected wall run.
The captures establish geometry, palette selection and ordering
for the unlit wall pass. Exact daytime or gameplay color, roof
visibility, fading and dynamic occlusion remain unverified.
[Published gameplay reference](https://lilura1.blogspot.com/2020/06/Arcanum-Walkthrough-Guide-Part-X-Tarant-Gilbert-Bates-Estate-Persuasion-Infiltration-Disguise-Stennar-Rock-Cutter.html)

## Follow-up with the CE gameplay capture

The supplied 1920 × 1080 CE capture matches the internal map
`Bates Mansion Lev 2`. The native preview above deliberately uses
`Bates Mansion Lev 1`. These are separate entries (5012 and 5011)
in `rules/maplist.mes`. Both have the same Rug3-Flip pattern beside
a corner. The rug matches at 1.8× scale with a translation of about
(+233, +187) screenshot pixels relative to the native drawable.
It establishes camera registration and cannot identify the floor
on its own.

The winning Lev 2 sector `67108865.sec` comes from
`modules/Arcanum.PATCH1`. Its Rug3-Flip starts at (97, 81) and its
corner is at (94, 78). The corresponding Lev 1 coordinates are
(101, 81) and (98, 78). Registering each map on its rug gives this
comparison. Displacements are in native drawable pixels, with
positive X right and positive Y down.

| Landmark | Lev 1 base sector | Lev 2 sector | Lev 2 screen displacement |
|---|---|---|---|
| Bookshelf outer end, screen left | (103, 78), record 38 | (97, 78), record 29 | (+80, −40), two tiles |
| Bookshelf corner-side end, screen right | (100, 78), record 34 | (95, 78), record 27 | (+40, −20), one tile |
| Interior corner | (98, 78), records 31–32 | (94, 78), records 24–25 | (0, 0) |
| Near doorway post | (98, 80), record 60 | (94, 79), record 50 | (−40, −20), one tile |
| Far doorway post | (98, 85), record 99 | (94, 84), record 84 | (−40, −20), one tile |

Lev 1 uses four `p4` bookshelf pieces. The plain-wall neighbors at
both bookshelf junctions, records 33 and 39, already appear in
the preview. Lev 2 uses the three `p3` pieces followed by two
plain-wall cells and five `p5` cabinet pieces. Lev 2 also has the
corner scenery record at (94, 78) and uses a different mirrored
base-wall variant there. The doorway uses the same `d6` frame family
on both maps. The inspected shelf and doorpost instances use the
same prototype 1021, zero effective offsets and 100% scale. Their
rotation and ART pivot relationships follow the rules above.

An independent reconstruction of the Lev 2 sector and ART matches
the screenshot's bookshelf rail and base trim on both sides, both
doorposts and the nearby plank texture. In local comparisons of
high-pass luminance with the common camera transform, the Lev 2
landmarks reached correlations of 0.81–0.93 with residual
translations of at most one screenshot pixel on each axis.
The upper-corner test has less texture and is shared by the two
layouts. The Lev 2 plank variants also fit the screenshot after
the rug is aligned. Lighting, scenery and HUD pixels differ in
the unlit reconstruction.

The known CE wall overlap near the corner remains valid. It
cannot turn a four-piece bookshelf into the observed three-piece
sequence or shift both doorposts by one tile. The Lev 1 preview's
bookend and doorway positions agree with its sector.
The first gameplay capture verifies the Lev 2 layout. The later
`screenshot-2026-11-23_07-38-47-arcanum-ce.png` provides a direct Lev 1
comparison with `bates-wall-run-full-800x600.png`.

The enlarged comparison sheets and the registration measurements
are in `target/visual-checks/milestone-34/`. The CE panel is
resampled into the native coordinates. The Lev 1 panel is a crop
from the existing Wayland capture. The Lev 2 panel is labeled as a
diagnostic reconstruction.

### Direct Level 1 comparison

Four visible patches of the rug establish the common image transform.
In capture pixels, the CE position is 1.8 times the native position
plus approximately (326.4, 46.9). Local matches use high-pass
luminance to reduce sensitivity to lighting. Positive residuals
indicate that the CE landmark lies right or below its predicted
position. Values are estimates from raster sampling.

| Landmark | CE residual X, Y (capture pixels) |
|---|---:|
| Bookshelf outer top rail | +0.02, −0.31 |
| Bookshelf inner top rail | −0.16, −0.19 |
| Bookshelf outer lower trim | +0.31, −0.12 |
| Bookshelf inner lower trim | +0.08, −0.25 |
| Interior corner upper rail | +0.16, −0.30 |
| Interior corner base molding | −0.19, −0.41 |
| Doorway near foot | −0.12, −0.34 |
| Doorway far upper junction | −0.04, −0.22 |
| Doorway far upright, above the candle | +0.08, −0.09 |

The shelf bookends, corner and doorway align within half a CE capture
pixel on either axis. The far post's foot is obscured by the candle
stand in CE. Additional flooring visible past the doorway lies beyond
the native preview crop. Scenery and light modulation account for the
other visible differences in this section. The matching rail and base
molding confirm that the selected wall records cover both bookshelf
junctions. The comparison supports the current placement and draw
order in this view. No renderer changes were made.

The read-only `Slot0036` save index contains a 1,812-byte
`maps/Bates Mansion Lev 1/67108865.dif` entry. Its flags select object
differences (`0x0008`), with no tile, light-list or roof override. The
screenshot comparison checks the resulting visible geometry directly.
This check did not classify each saved object-field change or establish
that the later capture came from that exact save state.

The two `bates-lev1-*-comparison.png` sheets and the registration
overview in the visual-checks directory are derived diagnostic crops.
They retain original colors and label the resampled CE panel.

## Performance and regression results

An optimized Rust 1.100.0 probe on Linux used a single pinned
i5-6300U logical CPU, one warm-up batch and seven measured batches.
The parse and decode rows use preloaded bytes. VFS group import
includes sector framing and inflation, name and prototype
resolution, palette decoding and mirroring. The draw probe uses
preallocated 800 × 600 buffers and excludes window presentation.

| Stage | Repetitions per batch | Median µs [min, max] |
|---|---:|---:|
| Parse all 368 sector object records | 1,000 | 65.8 [62.1, 68.6] |
| Decode 13 distinct wall frames with their selected palettes | 800 | 282.4 [279.8, 293.8] |
| Resolve and load the 17-wall batch through the mounted VFS | 150 | 5,035.9 [4,896.9, 5,315.9] |
| Draw one wall | 250 | 9.44 [7.17, 11.22] |
| Draw 17 walls | 250 | 141.2 [136.8, 182.3] |
| Compose 81 surface cells and 17 walls | 250 | 538.3 [513.6, 566.5] |
| Clear, compose and pack | 140 | 2,194.9 [2,153.6, 2,288.6] |

The batch retains 13 decoded frames from 12 paths, totaling
289,760 RGBA pixel bytes. The 800 × 600 RGB8 framebuffer
and one packed buffer use about 3.20 MiB before backend overhead.

All 138 portable tests and five headless demonstrations ran with
network and assets isolated. All 21 installed-data checks passed,
including `vfs-smoke`. Both schematic audits returned 119/119.
Formatting, Clippy, default and windowed tests and builds, release
build and diff checks passed.

## Next milestone

Import one original static portal at the door opening and check
its ART pivot and same-tile ordering against the adjacent walls.
Keep door interaction and dynamic occlusion outside that slice.
