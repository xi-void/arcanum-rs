# Bounded Bates Mansion scenery

The room preview uses world tile (101, 81) as its center. It draws
a 9 × 9 original floor patch with 15 baked rug frames, 17 walls,
and seven Scenery records. The overview uses a larger contiguous
patch from the same base sector. Its selection uses the original
tile and raster bounds of each object.

## Original room records

The winning sector is `maps/bates mansion lev 1/67108865.sec` in
`modules/Arcanum.dat`. All seven records have serialized type 3
(Scenery). The bookshelf is part of the Wall artwork. The round
table, sofas, candle stands and paintings are separate Scenery
records. The selected prototypes come from the winning loose
`data/proto` mount. All seven ART resources come from
`Arcanum2.dat` under `art/scenery`.

| Record | World tile | Prototype | Current ART ID and file | Pixel offset | ART hotspot | Object and scenery flags |
|---|---|---:|---|---|---|---|
| 35 | (100, 78) | 4032 | `401001c0` candlestand1.art | (+8, −3) | (16, 78) | `30`, `20` |
| 40 | (104, 78) | 4041 | `40580400` painting02_r.art | (−14, +7) | (16, 117) | `404830`, `0` |
| 53 | (98, 79) | 4041 | `40400400` painting01_l.art | (+14, −2) | (20, 115) | `404830`, `0` |
| 61 | (103, 80) | 4039 | `41200380` couch_flip.art | (+23, −8) | (61, 70) | `404830`, `0` |
| 70 | (101, 81) | 4037 | `40200300` gc_table.art | (−8, −15) | (22, 51) | `404c30`, `0` |
| 78 | (100, 82) | 4039 | `41180380` couch.art | (+12, −2) | (70, 70) | `404830`, `0` |
| 100 | (98, 85) | 4032 | `401801c0` candlestand2.art | (−5, −6) | (19, 76) | `30`, `20` |

All seven inherit 100% scale, neutral blit color, blit flags 0
and alpha 255. They select rotation 0, frame 0 and palette 0.
The fields select visible, upright objects with ordinary blending.
The candle stands' subtype bit `0x20` marks respawnability in CE.

## Format and composition

The sector reader reuses the existing variable-length object
framing after the light, floor, roof and metadata sections.
An explicitly stored instance field, including zero, takes
precedence over the version 119 prototype. Scenery prototypes
serialize 36 common slots and six subtype slots. The reader
validates the prototype's identity, type and framed record length.

The scenery branch of CE's `src/game/name.c` looks up
`1000 × family + number` in `art/scenery/scenery.mes`. TIG's
`tig_art_scenery_id_create` defines number in bits 19–27, frame
in 14–18, rotation in 11–13, family in 6–10 and palette in 4–5.
The sofa orientations select separate one-direction ART files.
Decoded frames are reused by resolved path, frame and palette.

CE's `object_get_rect` places the projected tile center plus the
signed instance pixel offsets at the stored ART hotspot. Frame
movement offsets remain metadata. The floor, walls and scenery use
the same center and projection. The overview selects supported
objects by raster intersection with the fixed canvas, including
objects on tiles outside the selected floor. Unsupported appearances and resources that
fail resolution or decoding produce diagnostics.

CE's `object_draw` visits tile rows and assigns flat objects a
lower drawing pass than upright objects. For ordinary upright
objects on the same tile, `sub_4F20A0` and `sub_4B93F0` order by
`(v + h, v − h)`, with `h = (offset_x − 40) / 2` and
`v = 2 × (offset_y / 2)`. The divisions truncate toward zero.
Walls keep their verified rotation-dependent ordering offset.
Equal keys retain original insertion order. The bookshelf-side
stand at (100, 78) draws after its wall. The installed regression
checks opaque coverage and wall pixels visible through the stand.

## Furnished room visual check

The captures and registered comparisons are under
`target/visual-checks/milestone-35/`. The 400 × 400 Wayland capture
matches the central crop of the 800 × 600 view pixel for pixel.
The client issued no additional buffer attaches during three
seconds of idle time at either size. The resize added one attach.

The supplied CE Level 1 screenshot
`screenshot-2026-11-23_07-38-47-arcanum-ce.png` uses the prior rug
registration at scale 1.8 and approximately (+326.4, +46.9) CE
pixels of translation. Rug texture, the interior corner rail and
bookshelf trim provide independent control patches. With that
alignment fixed, the visible table, both sofas, two candle stands
and two paintings match to within 0.35 CE pixels per axis.
The right sofa has a lower-confidence match because actors and
lighting obscure part of it. Lighting, shadows, actors, fire and
flooring outside the small patch account for visible differences.

The preview uses the base sector. The object-only `Slot0036`
sector override remains reference evidence.

## Furnished room release timings

Linux x86-64, Intel i5-6300U, Rust 1.100.0. An optimized warm-cache
probe pinned to one logical CPU used seven timed batches. Timings
are medians with the range of batch averages in parentheses.
Sector VFS read took 174 µs (167–244), object parsing 105 µs
(90–111), the seven scenery ART VFS reads 557 µs (530–1,007),
and decoding their frames 82 µs (78–84). The complete import
took 19.82 ms (15.55–35.11) with the VFS already mounted.
Seven scenery blits took 65 µs (58–68). Full 800 × 600 clear,
compose and pack took 2.03 ms (1.97–2.53), excluding presentation.
The seven decoded sprites retain 160,832 bytes of RGBA data.

## Level 1 overview

`art sector-room-wide` opens the 1280 × 720 preview.
`art sector-room-wide-inspect` reports its selected resources and
omissions. It loads a 25 × 25 patch covering world X 89–113 and Y
71–95, centered on (101, 83). All cells lie in the same original
sector and resolve through the VFS. The 625 surface draws include
the adjoining hall's original floor and baked carpet. The
overview selects 96 walls and 24 scenery records by raster
intersection, including 19 walls and seven scenery records whose
tiles lie outside the floor patch. The original sector has 268
walls, 19 portals and 81 scenery records in total.

The wider wall set adds the verified undamaged exterior facing
(rotation 5), which selects the second family in the structure
record. It also uses additional base, door and bookshelf pieces
whose suffixes come from CE's `a_name_wall_aid_to_fname` and
`build_wall_file_name`. The existing wall projection, pivot
rules, palette handling and object ordering apply to these
records. None of the doorway offsets changed.

The diagnostic lists 11 nearby portals and 13 nearby scenery
records without supported rendering. Two are additive-blended
fires. The other 11 are chair records requiring multidirectional
ART or palette slot 2. There were no skipped wall records in the
reported area. Portals can change the appearance of open
doorways. Walls also remain visible where CE may hide or fade
them. The overview has no dynamic visibility, lighting, shadows,
actors, or save overrides. Its fixed raster selection clips to
the designated preview.

The overview comparison reuses the earlier registration with
CE pixel position `1.8 × native + (38.4, 10.9)`. The new
translation follows from the shared tile-center shift and
framebuffer dimensions. This registration gives these residuals
in CE capture pixels.

| Landmark | X, Y residual |
|---|---:|
| Furnished room rug | +0.01, +0.46 |
| Furnished room bookshelf trim | +0.41, +0.16 |
| Interior corner base molding | −0.31, −0.12 |
| Adjoining doorway near foot | −0.20, −0.06 |
| Second bookshelf upper rail | +0.19, +0.13 |
| Second bookshelf lower trim | −0.06, −0.01 |
| Planks through the adjoining doorway | −0.24, +0.15 |
| Farther hallway planks | −0.38, +0.08 |

The matching features span the sitting room and the hall beyond
the doorway. The supplied CE image does not cover the overview's
full dining and staircase areas. Those parts rely on verification
against the original sector and ART records. No additional
Wayland buffer attach occurred during the three-second idle check
at 1280 × 720.

With the VFS already mounted, the overview imported in 44.35 ms
(41.61–50.64 ms, 15 repetitions per batch). The 745 ordered draws
reference 109 distinct rasters. Reusing the draw list and buffers,
1280 × 720 clear, compose and pack took 7.23 ms (6.94–9.47 ms,
120 repetitions per batch). The figures exclude Wayland submission.

### Black outside the room

The 1280 × 720 RGB framebuffer is cleared to (32, 43, 58) on each
redraw. In a replay of the overview, all 106,616 pixels without an
opaque draw retained that value. The source attribution below uses
the RGB buffer before Wayland presentation. The desktop capture can
sample raster edges differently.

| Final source of RGB (0, 0, 0) | Pixels |
|---|---:|
| Original black base tiles | 84,997 |
| Original dirt/black transition tiles | 50,827 |
| Opaque black texels in wall ART | 24,318 |
| Opaque black texels in scenery ART | 2,877 |
| Opaque black texels in rug facades | 137 |
| Clear or unowned pixels | 0 |

The 25 × 25 patch of `67108865.sec` includes 145 instances of
`0x071c01c0`, which resolves to `art/tile/blkbse0a.art`, and 84
instances of dirt/black transition IDs. For example, tile (111, 80)
stores `0x001c61c1` and resolves to horizontally flipped
`art/tile/drtblk2a.art`. Both resources come from `Arcanum2.dat`.
Their frame 0 is 78 × 40 with hotspot (39, 39). An independent
read of the RLE data found 1,600 texels at opaque black palette
index 254 and 1,520 texels at transparent index 0 in each. The
eight dirt/black ART files selected by the patch are byte-identical
to one another. These tiles form the authored black void around
the building. Skipping an unsupported object leaves the existing
underlying pixel visible.

All visible source frames use alpha 0 or 255. Transparent edges
preserve the underlying pixel. Three clipped replays crossing
exterior walls matched their respective crops of the full RGB
frame byte for byte. In the registered CE Level 1 capture, both
the black strip and its junction with the left bookshelf wall
are visible. The first colored wall pixel at six vertical
transects fell within one CE capture pixel of the projected
native edge. The CE framing and HUD limit comparison of the
foreground and the farther perimeter.

All floor pixels in the 1280 × 720 viewport could be supplied
by tiles in the current sector, covering world X 84–118 and
Y 66–100. Extending floor coverage within that sector would
replace 64,800 of the current clear pixels with authored black
and 41,816 with colored artwork. The eight surrounding sectors
in the same map archive have no serialized lights or objects.
Their 32,768 tile references all resolve to black base or
dirt/black ART whose opaque texels are black. Loading those
sectors would extend the void. No rendering changes were needed.
