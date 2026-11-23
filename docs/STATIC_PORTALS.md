# Static portals in the world preview

`art world` draws the supported base-sector floor and facade
cells, walls, scenery and portal leaves together. Its default
Bates Mansion Level 1 region keeps the 25 × 25 footprint and
(101, 83) center of `art sector-room-wide`. The earlier preview
remains a portal-free regression fixture. The world loader
accepts one sector, local floor origin and size, world tile
center and viewport dimensions from the caller. Its CLI fixes
the map and sector to Bates Mansion Level 1 and exposes the
remaining parameters in `--view`.

```sh
cargo run --locked --features windowed -- art world "$game"
cargo run --locked -- art world-inspect "$game"
cargo run --locked --features windowed -- art world "$game" \
  --view 25 7 25 96 90 1000 720
```

## Base-sector inventory

The winning file is `maps/bates mansion lev 1/67108865.sec` from
`modules/Arcanum.dat`. All 19 serialized Portal records in this
sector are doors. The 11 records identified as near the overview
are listed below. Their record bitmap stores only the current
ART word and location. All 11 reference serialized prototype
2028, from the winning loose `data/proto/002028 - Portal.pro`.
Its own ART word is `0x30003411`. The instance's stored current
ART word supplies the displayed state. Every record inherits
offset (0, 0), 100% scale, object flags `0x4000`, portal flags
`0x9`, neutral blit color, blit flags 0 and alpha 255.

| Record | World tile | Stored current ART | ART filename | Rotation / frame / palette | Default overview |
|---|---|---|---|---|---|
| 58 | (86, 80) | `0x32080c00` | in4e3au1.art | 1 / 0 / 0 | Outside viewport |
| 105 | (86, 86) | `0x32080c00` | in4e3au1.art | 1 / 0 / 0 | Outside viewport |
| 142 | (89, 90) | `0x32203c11` | in4e4bu0.art | 7 / 0 / 1 | Outside viewport |
| 150 | (94, 90) | `0x32103c11` | in4e4au0.art | 7 / 0 / 1 | Drawn |
| 152 | (95, 90) | `0x32203c11` | in4e4bu0.art | 7 / 0 / 1 | Drawn |
| 184 | (98, 93) | `0x32100c00` | in4e4au0.art | 1 / 0 / 0 | Drawn |
| 186 | (110, 93) | `0x3178ec00` | gb1e4au0.art | 5 / 3 / 0 | Open, drawn at viewport edge |
| 191 | (98, 94) | `0x32200c00` | in4e4bu0.art | 1 / 0 / 0 | Drawn |
| 193 | (110, 94) | `0x3198ec00` | gb1e4bu0.art | 5 / 3 / 0 | Open, drawn at viewport edge |
| 244 | (103, 98) | `0x32103c11` | in4e4au0.art | 7 / 0 / 1 | Drawn |
| 246 | (104, 98) | `0x32203c11` | in4e4bu0.art | 7 / 0 / 1 | Drawn |

The closed files resolve under `art/portal` in `Arcanum2.dat`.
The default world view draws six closed leaves and two open leaves.
The 11 remaining closed leaves are reported as culled.
The cache key contains the VFS path, frame, palette and mirror.
The static staircases use the existing scenery path.

## Format and placement evidence

The portal instance reader reuses sector object framing and
reads the supported common appearance bits and portal flags.
Version 119 prototypes serialize the 36 common slots and eight
portal slots. The last two portal slots use framed four- and
eight-byte-element arrays. Prototype identity and category,
bounds, field masks and record lengths are validated. Instance
values take precedence when explicitly present, including zero.

TIG's `tig_art_portal_id_create` defines type nibble 3, number
bits 19–27, frame 14–18, rotation 11–13, door bit 10, damage
bit 9 and palette bits 4–5. Bit 0 signals horizontal mirroring.
CE's `a_name_portal_aid_to_fname` uses the first token from
`art/portal/portal.mes`, keyed by the ART number for doors and
number plus 1001 for windows. Damaged artwork has a separate
filename convention. CE's `portal.c` identifies frame 0 as
closed. The rotation-5 leaves above store the open animation's
frame 3. The native preview draws the verified undamaged
closed-door variants for rotation 1 / palette 0 and rotation
7 / palette 1 and the verified fully-open variant at frame 3,
rotation 5, palette 0, with mirroring disabled. Other nonzero
frames, windows, damaged appearances, and non-default scaling
or blending produce explicit diagnostics.

The portal ART uses one stored direction and seven frames.
TIG's `tig_art_frame_data` uses the same directional hotspot
adjustment for walls and portals. Rotations 1 and 7 apply
(−40, +20), followed by `width − adjusted_x − 2` for a
mirrored X pivot. The native loader mirrors the decoded rows
once and retains the ART hotspot and movement metadata.
`object_get_rect` places the adjusted pivot at the projected
tile center plus the resolved pixel offset. Records 150 and
152 have 33 × 96 frames with stored hotspots (29, 79) and
(38, 83) and effective pivots (42, 99) and (33, 103).
Their 1280 × 720 raster origins are (1158, 261) and (1127, 277).

CE's `sub_4F20A0` inserts a normal portal immediately after the
same-tile wall. The importer requires one matching opening and
the same rotation. The shared draw order places each rendered
leaf after its paired wall at the same tile and depth, then
continues the existing object order. Records 150 and 152
follow wall records 149 and 151. Neighboring scenery retains
its existing flat/upright pass and offset ordering.

## Visual verification

The genuine Wayland full, close-up and resized captures are
under `target/visual-checks/milestone-36/`. The 1280 × 720 and
1160 × 600 windows submitted zero additional buffers during
three idle seconds at either size. An installed-data test
checks paired ordering, frame-cache reuse and exact crop
equivalence through the shared compositor.

The earlier CE Level 1 capture covers the furnished room and
rug-side opening. Its rug, trim and floor controls remain
within 0.46 CE pixels per axis. The later supplied hallway
capture shows the closed pair 150/152 beside the portrait and
the neighboring pair 140/142. A native RGB replay uses the
same world loader and compositor, with local origin (20, 15),
center (94, 94) and a 1100 × 720 viewport for the comparison.

The portrait frame, candle stand, wall rail, clear floorboards,
corner molding and carpet edge establish the transform
`CE = 1.8 × native + (−274.66, +208.71)`. With that registration
held fixed, the high-pass luminance matches give these residuals
in CE capture pixels. Positive X is right and positive Y is down.

| Portal record | X, Y residual |
|---|---:|
| 150 | −0.18, +0.11 |
| 152 | −0.20, +0.09 |
| 140 | −0.31, 0.00 |
| 142 | +0.01, −0.05 |
| 177 | −0.23, +0.29 |
| 196 | −0.33, −0.17 |
| 105, partly clipped | −0.03, −0.06 |

The shared frame, panel and glazing geometry supports the
original closed state, orientation and pivot. CE's lighting,
actors and foreground wall fading account for visible
differences in the registered views. The replay uses the
base sector. The supplied capture provides no same-door
verification for the two open leaves, 186/193.
The source capture, registered comparisons and measurements
are under `target/visual-checks/milestone-36/`.

## Milestone 37 entrance pair

Records 186 and 193 occupy (110, 93) and (110, 94), directly
after wall records 185 and 192. Their spans are 34891–34974 and
35516–35599 (half-open) in sector `67108865.sec`. The effective
ART words are the stored instance words `0x3178ec00` and
`0x3198ec00`. The prototype's ART word remains `0x30003411`.
Each instance overrides only current ART and location. It
inherits the same offsets, scale, colors and flags as the
closed leaves. The raw flags describe the object and prototype
properties. The stored frame supplies its visual state.

| Record | VFS winner | Frame 0 dimensions / hotspot | Frame 3 dimensions / hotspot | Frame 3 opaque / transparent texels |
|---|---|---|---|---|
| 186 | `Arcanum2.dat`, `art/portal/gb1e4au0.art` | 33 × 95 / (26, 79) | 20 × 81 / (26, 80) | 1,209 / 411 |
| 193 | `Arcanum5.dat`, `art/portal/gb1e4bu0.art` | 34 × 94 / (36, 82) | 41 × 80 / (40, 68) | 158 / 3,122 |

An independent read of the winning ART payloads found seven
frames, one stored direction, palette slots 0 and 1, and zero
movement offsets on frames 0 and 3. Frame 3 is RLE compressed
in both files and treats index zero as transparent. The frame 3
pair shows different silhouettes because the leaves swing
differently in these authored frames. No change to either
pivot or depth rule is needed. CE normalizes direction 5 to 4
for its opening sequence and stops at frame 3. The shared pivot
rule leaves the rotation-5 hotspot unchanged. Raster origins
in the default 1280 × 720 frame are (654, 660) and (680, 692).
Both leaves draw directly after their same-tile wall before
existing object ordering continues. The reprojection
close-up changes only the shared center.

`world-inspect` now reports drawn and culled portal states and
unsupported records across the selected sector. The closed-only
fixtures remain available. Wayland captured the default
1280 × 720 overview, a recentered entrance at 960 × 640, a
close-up and a 500 × 300 resized view. The overview and both
entrance sizes submitted zero additional buffers during their
three-second idle checks. The two 1280 × 720 Wayland overviews
from Milestones 36 and 37 differ at 1,061 pixels. Every changed
pixel lies in the entrance area, X 654–717 and Y 659–719.
The rest of the images match exactly. These captures and source
diagnostics are under `target/visual-checks/milestone-37/`.

The supplied CE hallway capture provides an exact closed-door
comparison. The available CE screenshots do not show the same
open entrance pair. The open leaves were checked against the
original bytes and CE's frame, pivot and insertion rules.
There is no gameplay screenshot basis for comparing their
open-state lighting or wall fading.

## Milestone 36 release measurements

On the Milestone 35 Linux host with an i5-6300U, an optimized
warm-cache probe pinned to one logical CPU took the median of
seven batches. Sector object parsing took 62.45 µs, six portal
names and VFS lookups 32.91 µs, and decoding and mirroring the
four visible variants from memory 17.20 µs. Six portal blits
took 27.47 µs. Frame timings reuse the loaded draw list and
buffers and include clear, compose and pack.

| 1280 × 720 composition | Median | Batch min–max |
|---|---:|---:|
| Earlier Milestone 35 measurement | 7.23 ms | 6.94–9.47 ms |
| Current regression preview, same run | 6.330 ms | 6.252–6.439 ms |
| Current world preview | 6.334 ms | 6.301–6.488 ms |

The same-run frame ranges overlap. Wayland submission is outside
these measurements. Portal frames are decoded during scene
loading and reused across redraws.


## Milestone 37 release measurements

With the same pinned i5-6300U setup and 1280 × 720 dimensions,
a rerun of the Milestone 36 binary took 7.102 ms for full-frame
clear, compose and pack. A paired comparison in the current
binary alternated between the same draw list with the two
open leaves withheld and with them included. One warm-up pair
preceded seven batches of 100 frames per variant.

| Variant | Median | Batch min–max |
|---|---:|---:|
| Same-run closed-only control | 6.845 ms | 6.753–6.995 ms |
| Open and closed | 6.901 ms | 6.732–6.996 ms |
| Two open-leaf blits alone | 2.9 µs | 2.5–4.0 µs |

The full-frame ranges overlap. These measurements exclude
presentation and loading. The decoded open frames are cached
by the existing path/frame/palette/mirror key.

The Rust CI workflow runs formatting, Clippy, portable tests
and headless and windowed builds on GitHub. Installed-data
verification remains local.
