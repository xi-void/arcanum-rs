# Multi-sector world navigation

The windowed world viewer can stream a bounded rectangle of original map
sectors. The starting position is a world tile. The four sector bounds
are inclusive and use sector coordinates.

```sh
game="$HOME/Work/arcanum-ce-test"
cargo run --locked --features windowed -- art world "$game" \
  --sectors "k'na tha" 5 14 8 14 341 928 1280 720 --trace
cargo run --locked -- art world-inspect "$game" \
  --sectors "k'na tha" 5 14 8 14 384 928 1280 720
```

Arrows pan, Shift increases the step, Home restores the starting point and
Escape closes the window. The default Bates Mansion view and the existing
`--view` inspection mode are also available.

The current viewer uses [predictive sector preparation](SECTOR_PREFETCH.md).
The measurements below preserve the Milestone 39 synchronous baseline.

## Original data

CE's `sector_id_from_loc` shifts each world tile coordinate right by six.
`SECTOR_MAKE` packs the X sector into bits 0–25, then the Y sector into bits
26–51. `sector_loc_from_id` recovers the world-tile origin by shifting both
components left by six. `sector_rect_from_loc_rect` splits the footprint
at multiples of 64. The decimal packed ID is the stem of each base
`maps/<map>/<id>.sec` resource. The Rust viewer uses that convention
through its VFS.

| K'na Tha sector | Origin | Source | Floor cells | Original walls and scenery | Renderable walls and scenery |
| --- | --- | --- | ---: | ---: | ---: |
| (5,14), 939524101 | (320,896) | modules/Arcanum.dat | 4096 | 28 / 433 | 28 / 386 |
| (6,14), 939524102 | (384,896) | modules/Arcanum.dat | 4096 | 64 / 475 | 64 / 426 |

The original floor arrays begin at byte 4 after a zero light count in
both samples. At world Y=928, the seam cells at X=383 and X=384 store
`0x004109c0` and `0x004101c0`. The winning floor frames are
`art/tile/grsbse0e.art` and `art/tile/grsbse0a.art` from Arcanum2.dat.
The two selected sectors contain no baked facades or portals. The Bates
regression covers the existing facade and portal paths.

The table records the Milestone 39 baseline. Milestone 41 resolves the
alternate tree palette requests and the prototype ART for positioned
scenery with no instance ART override. The
[coverage and location audit](SCENERY_ART_COVERAGE.md) gives the updated
counts and the coordinates of the original paved brazier court.
The headless inspection reports unsupported inputs. A sector whose object list cannot be framed
keeps its validated floor and reports the object-layer error. An absent
sector or unsupported floor cell leaves the normal cleared background.
Original black void tiles remain regular original draws.

The streamed viewer also supports opt-in [original static lighting](STATIC_LIGHTING.md)
at a fixed scheme hour, with mask preparation in the same sector worker
and shared shading for floors, scenery, walls and portals.

## Streaming and ordering

The camera keeps one pixel transform relative to the initial world tile.
Before drawing, inverse projection maps the viewport into the sector
grid with a conservative 640-pixel overhang allowance on both screen
axes. Static rasters outside that documented owner-tile bound are
reported. Final culling tests each actual raster rectangle, including its
pivot and instance offset.

Each sector has one owner for its cells and original records. Nearby
sector draws are merged in a single stable world order whenever the
active sector set changes. Surfaces render in world Y/X order, then flat
and upright objects use the same depth and tie-break rules as the single
sector viewer. Baked facades remain in the surface pass. No loading
occurs inside the compositor.

The sector cache retains at most 12 entries, including unavailable
sectors. It evicts the least recently used entry outside the required
set. A viewport whose conservative footprint needs more than 12
sectors receives an explicit error. A separate decoded ART cache uses
resource path, frame, direction, palette and mirroring as a key. It shares frames
across sectors and is capped at 96 MiB of retained RGBA and 4,096
frames. Only entries that no cached raster references can be evicted
from the ART cache.

`--trace` reports sector loading, ART resource reads and decoding,
global ordering, software composition, packing and client-side
submission. Slow frames are reported without enabling per-frame
tracing. These durations distinguish client work from an unexplained
stall. They do not measure an asynchronous compositor's display
latency.

## Verification

In a native 1280 × 720 Wayland window the path began at world tile
(341,928), crossed at the exact X=383.5 boundary and continued to X=402.7.
Returning Home after a 960 × 560 resize reproduced all 921,600 original
pixels. The Bates Mansion capture shows original geometry beside its
black void boundary. Stationary intervals showed no new buffer
attachments.

Portable fixtures cover a populated seam, cross-sector wall layering
and transparency, viewport overhang, missing sectors, warm reuse and
sector eviction. Installed tests cover K'na Tha's two original sectors,
world-space overlap during crossing and the Bates Mansion 268-wall
regression.

## Release measurements

This run used Rust 1.100.0 on a Linux ThinkPad with an i5-6300U, pinned to
CPU 1. The VFS and filesystem page cache were already warm. The first
sector took 20.73 ms to import and the neighboring cache miss took
13.43 ms. Ordering took a further 0.60 and 1.16 ms respectively.
The new neighbor required 11 additional VFS streams, including two ART
resource reads, and three frame decode attempts.

The warm benchmark rendered 250 frames at 1280 × 720, moving repeatedly
across the same seam. Composition includes the visibility pass,
framebuffer clear and packing, with window submission measured separately
in the Wayland trace.

| Warm operation | Median | p95 | Worst |
| --- | ---: | ---: | ---: |
| Camera and sector preparation | 0.0014 ms | 0.0060 ms | 1.455 ms |
| Visibility selection alone | 0.119 ms | 0.349 ms | 0.510 ms |
| Complete software composition | 7.227 ms | 8.676 ms | 10.329 ms |

The warm route performed no sector reads, VFS streams or ART decodes.
Two cached sectors used 2.16 MiB of RGBA and 2.72 MiB of raster descriptors,
excluding VFS indices, diagnostics and framebuffers. A sweep across
sector X=5 through X=18 at Y=14 reached the 12-entry cap. It evicted two
sectors outbound and one on returning Home. Reloading Home took
12.79 ms of import time and used nine VFS streams, zero ART reads and
zero frame decodes. The extended sweep retained approximately 2.27 MiB
of RGBA and 6.76 MiB of raster descriptors after returning.

In the separate Wayland trace the slowest observed frame was the first
1536 × 864 configure. It took 76.73 ms in total, including 44.34 ms
of sector preparation. The trace records each stage independently.
The Milestone 38 outlier remains unattributed.
