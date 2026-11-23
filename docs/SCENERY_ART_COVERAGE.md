# Original scenery coverage and K’na Tha location audit

## Where the screenshots were taken

Sector columns and rows are the world tile coordinates divided by 64, rounded
down. The decimal sector filename encodes `sector_x | (sector_y << 26)`.
The original files are mounted from `modules/Arcanum.dat`. Native camera
coordinates and sector file identities agree with CE's `sector_id_from_loc`
and `sector_loc_from_id` and with the placements read from the original bytes.

| Landmark | Original map | World tile(s) | Owning sector |
| --- | --- | --- | --- |
| Forest Passage, CE screenshot at 10:41:46 | `arcanum1-024-fixed` | (91900,39315) | (1435,614), 41204843931 |
| Scripted arrival from the Passage | `k'na tha` | (802,854) | (12,13), 872415244 |
| Paved court braziers, CE screenshot at 10:42:09 | `k'na tha` | (793,781), (798,781), (803,781) | (12,12), 805306380 |
| Original streaming fixture | `k'na tha` | X=320–447, Y=896–959 | (5,14), 939524101 and (6,14), 939524102 |

The Passage is scenery record 160 in
`maps/arcanum1-024-fixed/41204843931.sec` (bytes 32296–32431). It uses
prototype 4047 and stored ART `0x40c80500`, resolved to
`art/scenery/tree_door.art` in `Arcanum2.dat`. The frame is 162 × 244 with
hotspot (72,200), offsets (+5,−15) on the object, and stored frame, direction
and palette zero. The door sprite occurs in three overworld sectors. This
placement's sparse script array has an entry at index 1 (`SAP_USE`) for script
2568. Its original `scr/02568throw_dung_to_map_1_tel.scr` contains a teleport
action with map key 5067 and tile (802,854), independently identified as K’na
Tha by the winning `rules/maplist.mes`. The other two placements point to
different map keys (5040 and 5018). The available saves were inspected
read-only and supplied no matching camera coordinates.

The court's original scenery records 102, 104 and 106 use `0x407002c0`,
`art/scenery/brazurelarge.art`. Records 103, 105 and 107 share those
coordinates, with `0x40100540` resolving to `art/scenery/brazurefire.art`.
At constant Y, each successive base is projected 200 pixels left and 100
pixels down relative to the previous base. The two seam-test sectors have
only grass, dirt and their transitions. Their original object lists contain
walls and scenery. The paved court and overworld Passage are outside them.

```sh
cargo run --offline --release --features windowed -- art world "$HOME/Work/arcanum-ce-test" \
  --sectors arcanum1-024-fixed 1434 613 1436 615 91897 39313 1280 720
cargo run --offline --release --features windowed -- art world "$HOME/Work/arcanum-ce-test" \
  --sectors "k'na tha" 12 12 12 12 798 786 1280 720
```

The Wayland captures were registered to the CE screenshots at native size
by scaling the CE reference by 1/1.8, then fitting a single translation per
view. For the Passage, the door and forest-wall patches yield (−54,−124)
for the translation from native to scaled CE coordinates. The stairs give
(−55,−124). The two exposed brazier bases independently give (+169,−102)
and (+170,−102). The water channel and wall contours agree at those
translations. The CE Passage image shows heavier foliage coverage over
the steps and a different ground appearance. The adjacent overworld sector
(1436,614) loads its original floor but rejects its object list at record
320 with an unsupported field bitmap. No particular missing adjacent object
has been tied to that visual difference. The court differs in flame effects,
creatures, lighting and shadows. None of those was used for registration.

## Scenery and artwork rules

| Sector | Serialized objects | Walls retained | Scenery retained, M40 → M41 | Portals retained | M41 rejections |
| --- | ---: | ---: | ---: | ---: | --- |
| Bates Mansion 67108865 | 368 | 268/268 | 65 → 79/81 | 19/19 | Two additive fires |
| K’na Tha 939524101 | 461 | 28/28 | 386 → 433/433 | 0 | Zero |
| K’na Tha 939524102 | 539 | 64/64 | 426 → 475/475 | 0 | Zero |

All 1,000 objects in the two K’na Tha fixture sectors have original
placement and resolve to a retained raster. There are no other records in
these two object lists awaiting dynamic behavior or ART resolution. At the
original 1280 × 720 seam view, sector 939524101 submits 22 object rasters
and culls 439, while sector 939524102 submits 51 and culls 488. These counts
measure viewport intersection and draw submission. Individual pixels can
still be transparent or occluded. At the court, 583 out of 716 serialized
objects yield retained rasters, with 64 submitted and 519 viewport-culled
at the documented camera. The remaining 17 walls, 113 scenery and 3
portals in that sector are reported individually by `art world-inspect`.

- **Bates chairs.** Fourteen records resolve to `city_chair1.art`. The
  original ART stores eight directions, one frame per direction and four
  palettes. CE's `sub_51B710` reads all direction tables before pixel data.
  The scenery ART ID determines direction and palette independently of its
  MES filename key. Installed tests compare selected chair rotations and
  colors against an independent decode, including dimensions and hotspots.
- **K’na Tha trees.** Eleven original `tree.art` references request palette
  2. The file stores one direction and palette 0 only. CE's normal palette
  selection falls back to slot 0 when the requested palette is missing.
  The decoder reports which direction and palette it actually used.
- **Inherited current ART.** The other 85 formerly unrendered tree records
  already contain world placement. Their instance bitmaps omit current ART.
  CE's `obj_field_fetch` selects the prototype field when the change bit is
  absent. The native importer uses the VFS-selected prototype, retains the
  effective ART ID and exposes the absent instance override separately.
  It does not invent a position for an instance missing its location.
- **Fire.** Bates scenery records 19 and 27 both reference `campfire.art`,
  with 20 frames, nominal 8 FPS and an original 56 × 62 frame-zero raster.
  Their effective blend flag is `0x10`. The court flame objects request
  the same additive mode. CE's `object_setup_blit` uses the original palette
  for additive software blits. TIG skips palette index zero and uses
  saturating per-channel addition against already drawn pixels. Order
  therefore matters. Milestone 45 implements this operation and frame
  playback in the streamed viewer. [Original animated fire](ANIMATED_FIRE.md)

The complete local inventory is generated under
`target/visual-checks/milestone-41/scenery-inventory.tsv`. Its 112 rows
cover the 16 previously omitted Bates records and the 96 K’na Tha records.
It records raw and effective ART, source bytes, prototype IDs and fields,
placement, frame, rotation, palette, dimensions, hotspots and disposition.
The larger landmark sectors have separate `world-inspect` reports.

## Streaming, pixels and timing

The world viewer uses the same projection, depth keys, cache bounds and
sector worker as Milestone 40. The direction is now part of the frame
cache key. Draws referring to a single-direction resource share its stored
direction. Viewport diagnostics use the same raster intersection as
composition.

Before/after comparisons at matching Bates and K’na Tha views found no
changed pixels outside the new scenery raster bounds plus a one-pixel
margin. The captures use fractional Wayland scaling. The six differences
outside the exact rectangles ranged from 8 to 22 pixels per image, all
within that margin. K’na Tha's Home return and the Bates Home return each
matched their initial capture exactly. The court itself is pixel-identical
to the pre-change native capture. It was already renderable at the correct
coordinates, apart from the explicit coverage limits above. Resizing to
960 × 560 and returning to 1280 × 720 preserved the world. The Wayland
trace showed no additional buffer attachments during the stationary
windows checked at the seam, neighbor, Home, court or Passage.

Release measurements on the ThinkPad X270 (Core i5-6300U) reused the
Milestone 40 test route and clear, compose and pack benchmark at 1280 × 720.
The current run was pinned to logical CPUs 0 and 1. Historical timings
vary with host load and are included for context.

| Measurement | Milestone 40 | Milestone 41 |
| --- | ---: | ---: |
| Cold synchronous seam preparation | 16.72 ms | 21.85 ms |
| Prefetched seam preparation and ordering on caller | 1.81 ms | 3.11 ms |
| Warm composition median / p95 / worst (250 frames) | 9.80 / 15.20 / 23.68 ms | 8.10 / 10.03 / 11.02 ms |
| Warm VFS reads / ART decodes | 0 / 0 | 0 / 0 |
| Decoded cache frames / retained pixel bytes | 85 / 2,267,344 | 88 / 2,789,000 |

The neighbor's preparation ran on the worker for 16.71 ms, including
0.23 ms sector read, 0.11 ms object parsing, 12.84 ms object import and
1.42 ms raster preparation. Within preparation, ART resolution, reading
and decode took 0.24, 0.31 and 0.04 ms. Cache insertion took under 1 µs.
The worker completed before the neighbor became visible. There was one
prefetch hit, zero held frames at the seam and zero synchronous loading
wait on the event thread. The previous 182.86 ms outlier did not recur
in this route and remains unexplained. The raw timings and Wayland traces
are under `target/visual-checks/milestone-41/`.
