# Predictive sector preparation

The windowed `art world --sectors` viewer uses one sector preparation
worker. To replay the K'na Tha crossing with diagnostics:

```sh
game="$HOME/Work/arcanum-ce-test"
cargo run --locked --features windowed -- art world "$game" \
  --sectors "k'na tha" 5 14 8 14 341 928 1280 720 --trace
```

Arrow keys, Shift, Home and Escape retain their existing behavior.
The synchronous importer remains available to `art world-inspect`
and the reference benchmarks.

## Planning and ownership

The viewer first requests its current inverse-projected viewport with
the established 640-pixel raster overhang. It then samples half and full
lookahead positions, up to 320 pixels in the latest arrow direction. The
worker accepts at most two additional sectors as speculative work. A
direction change replaces queued work. A required sector moves to the
front of that list and an already running sector is never queued again.

The worker owns the VFS and decoded ART cache exclusively. It transfers
one complete sector or a diagnosed unavailable result at a time.
Immutable ART frames use shared reference counting. The presentation
thread neither waits for a resource read nor acquires a lock held during
import. Wayland, window events, view ordering and composition stay on
the presentation thread.

The 12-entry sector cache evicts speculative entries first, then the
least recently used visited entry outside the required and active sets.
An obsolete in-flight import may finish and leave reusable ART in the
worker cache. Its scene is discarded when it no longer matches the
current request. The decoded ART capacity remains 96 MiB and 4,096 frames.
Shutdown cancels queued work and joins the worker after the event loop
returns.

The presenter keeps the last complete buffer while a required sector
is pending, including a fast jump to an uncached position. It uses
the new view once the full required set is ready. An unavailable
sector remains a diagnosed empty part of the scene. The ordinary
cleared background and original black void tiles stay distinguishable.
A speculative completion updates the cache without submitting a buffer.

## Reading the trace

`--trace` shows request priority, queue delay, worker time and cache
insertion. The worker reports base-resource lookup/read, floor parsing,
surface resolution, object-list parsing, original object import and
raster preparation. ART path resolution, resource reads and decoding
are nested timings within surface and object import. Frame logs give
mailbox scheduling, ordering, composition, packing and client-side
Wayland submission separately. View hold measures elapsed time waiting
for a complete view to become available while the event thread remains
free. It is separate from synchronous loading wait. Startup and a cold
jump can hold the view.

A prefetch hit means a speculative sector was cached before first
becoming required. A miss means a predicted sector became required
before it was ready. Initial or unpredicted required imports are
also counted as required misses.

[Original sector evidence and Milestone 39 baseline](MULTI_SECTOR_WORLD.md)

## K'na Tha performance

Release build on the ThinkPad X270 (i5-6300U), pinned to CPU 0 and CPU 1
on separate physical cores. The same-run comparison used a warm
filesystem cache, a fresh sector cache, a 1280 × 720 software
framebuffer, and 64 one-tile steps from X=339 to X=402 at Y=928.
The worker replay allowed up to 16 ms between steps and the warm
replay measured 250 frames. All frame timings here exclude Wayland
submission. Milestone 39's separately recorded results remain intact.

At the conservative boundary entering the neighbor at X=343 the
synchronous path spent 16.72 ms preparing the view. This included
0.25 ms base VFS lookup/read, 0.20 ms parsing, 2.38 ms surface
preparation, 11.73 ms object import, 0.91 ms raster preparation and
1.09 ms ordering. ART resolution, reading and decoding respectively
took 0.27, 0.33 and 0.03 ms within those stages.

The worker prepared the same neighbor in 14.83 ms, with 11 VFS
streams, two ART resources read and three frame decode attempts.
It was ready before the crossing. The event thread spent 1.81 ms
scheduling and merging its cached draws on that step. It performed
zero reads, parsing or decodes and the paced traversal held no frames
after initial startup.

| Software replay | Median | p95 | Worst |
| --- | ---: | ---: | ---: |
| Synchronous cold-route full frame | 9.82 ms | 13.25 ms | 43.66 ms |
| Prefetched cold-route full frame | 10.23 ms | 17.24 ms | 21.03 ms |
| Warm clear, compose and pack | 9.80 ms | 15.20 ms | 23.68 ms |

The warm route performed no VFS reads or ART decodes. The slower p95
in the prefetched run is part of the measurement. These figures
demonstrate removal of synchronous import from the crossing, with
variable composition and scheduling times.

Native Wayland replays match the Milestone 39 starting, exact-seam,
neighbor, resized and Bates Mansion void captures pixel for pixel.
A wide replay recorded three sector evictions at 12 resident entries,
11 prefetch hits and two late predictions. Returning to the evicted
Home sector reloaded its records on the worker using nine VFS streams,
zero ART reads and zero decodes. All 921,600 returned pixels matched.
The wide replay retained 2.27 MiB of RGBA and 6.76 MiB of raster
descriptors. No additional buffers were submitted during idle samples.

The Milestone 38 142.8 ms sample cannot be attributed from its original
measurements. A separately traced 1280 × 720 frame measured 182.86 ms,
including 67.44 ms in ordering and 111.57 ms in composition, with no
synchronous load wait. These are elapsed stage times, including any
host scheduling delays.
