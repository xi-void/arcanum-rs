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
| 186 | (110, 93) | `0x3178ec00` | gb1e4au0.art | 5 / 3 / 0 | Open appearance, unsupported |
| 191 | (98, 94) | `0x32200c00` | in4e4bu0.art | 1 / 0 / 0 | Drawn |
| 193 | (110, 94) | `0x3198ec00` | gb1e4bu0.art | 5 / 3 / 0 | Open appearance, unsupported |
| 244 | (103, 98) | `0x32103c11` | in4e4au0.art | 7 / 0 / 1 | Drawn |
| 246 | (104, 98) | `0x32203c11` | in4e4bu0.art | 7 / 0 / 1 | Drawn |

The rendered files resolve under `art/portal` in `Arcanum2.dat`.
The visible leaves use four frame variants across two files.
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
7 / palette 1. Windows, open and damaged appearances and
non-default scaling or blending produce explicit diagnostics.

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
verification for the two unsupported open leaves, 186/193.
The source capture, registered comparisons and measurements
are under `target/visual-checks/milestone-36/`.

## Release measurements

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
